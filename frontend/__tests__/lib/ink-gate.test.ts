import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import * as CHART_COLORS from '@/lib/chart-colors';
import * as DESIGN_TOKENS from '@/lib/design-tokens';
import { TYPE_SCALE } from '@/lib/type-scale';

/**
 * Gate for the inks that carry words (UI review 2026-11-14, ruling R4; U-04, U-08, DY-02, DY-06):
 *
 * - the ghost ink (`text-text-ghost`, about 1.9:1 by night and 2:1 by day) is for the padding
 *   zeros of a number (the leading zeros, the trailing zeros of a fraction) and for pure
 *   decoration (a separator, an arrow, the `#` of a comment) — never for a word or a figure;
 * - an ink is never thinned by an alpha modifier (`text-text-dim/60`, `text-aqua/70`): a quieter
 *   ink is a named key of the palette, pinned at 4.5:1 (`__tests__/lib/palette.test.ts`);
 * - "set back" (a row or a tile that is context, not the subject) is drawn in the dim ink, never
 *   with element opacity; an `opacity-*` below 1 stands only where nothing it fades is a word
 *   (an icon, a mark) or where the state is a transient pointer focus, each listed with its reason.
 *   A disabled control (`disabled:opacity-*`) is the one variant allowed by rule. `opacity-0`
 *   hides; it does not fade.
 *
 * The sites the gate tolerates are listed per file, in the ORDER they occur in it: the n-th site
 * of a kind in a file has the n-th reason. A line number would break on every lane that moves a
 * line; an order survives that, and an added or a removed site still fails (a site with no reason,
 * or a reason with no site), the message naming the path, the line and the class.
 *
 * The class scan reads class names only, so the same defects written as an SVG attribute or an
 * inline style passed it (lane E2: a fraction `<tspan fillOpacity={0.7}>`, gene text at
 * `fillOpacity=".85"`, a tick in `fill="var(--c-text-ghost)"`). The JSX scan below reads the JSX
 * itself with the TypeScript parser and asks two questions of every element that HOLDS WORDS:
 *
 * - is it faded: an `opacity`, `fillOpacity` or `strokeOpacity` attribute on an SVG or HTML
 *   element, or an `opacity` in its inline `style`, whose value can be below 1 — a literal in
 *   (0, 1), or a value the source does not settle (a name, a call, arithmetic), which counts as
 *   fading; `cond ? 1 : 0` shows or hides and does not fade;
 * - is it painted in an ink under text grade — the ghost, the quiet or the muted (`text-ghost`,
 *   `ink-quiet`, `ink-muted`; R4 is about use, not a name): a `fill` or `color` attribute (`stroke`
 *   too on SVG text), a style `color`, `fill` or `--ink`, or a `fill-text-ghost` /
 *   `fill-ink-quiet` / `text-ink-muted` class, whose value reaches one of them — a
 *   `var(--c-ink-quiet)` string, `color('text-ghost')`, or a constant of the file or of
 *   lib/design-tokens.ts / lib/chart-colors.ts that holds one, through `?:`, `??` and calls.
 *
 * An element holds words when it is SVG text (`<text>`, `<tspan>`, `<textPath>`) or when its JSX
 * children, as written, contain text: non-blank JSX text, an expression that is not JSX (a name or
 * a call React writes as text), or such an element further down, following `?:`, `&&`, `||`, `??`
 * and the JSX inside a `.map(…)`. `<title>` and the paint servers paint nothing. A self-closing
 * shape (a bar, a dot, a line, an area) holds no words, so a data-driven alpha on it is a mark's
 * strength, never flagged; only what holds text is judged.
 *
 * Limits: a component's own props (`opacity` on `<LegendLine>`, `color` on a swatch) are its API,
 * judged where they land on an SVG or HTML element in its own file; a self-closing component
 * inside a faded or faintly painted element counts as holding words (it may draw some); a `style`
 * built by a call is not read for opacity; constants imported from other modules than those two,
 * an ink carried in data (a part's `tone`, a tooltip's `tone` read from an object), a fill
 * inherited from a parent in another file, and the stylesheet's own rules (globals.css) are
 * outside the scan.
 */

const PADDING = 'padding zeros: the trailing zeros of an amount’s fraction';
const SEPARATOR = 'decoration: the separator between the crumbs or the parts of a line';
const ARROW = 'decoration: the aria-hidden arrow between two tiles of a mechanism chain';

/**
 * One allowlisted site: why it may stand, and its anchor, a short text that must appear on the
 * site's own source line (the variable, the literal, the aria attribute that makes it what the
 * reason says). The review's item G: with reasons alone, "n reasons for the n sites of a file, in
 * order", removing one allowed site and adding a new faded word in the same file passed.
 */
interface Allowed {
  reason: string;
  anchor: string;
}
/** Per file, in order: its sites of one kind, each with its reason and its anchor. */
type Allowlist = Readonly<Record<string, readonly Allowed[]>>;
const padding = (anchor: string): Allowed => ({ reason: PADDING, anchor });
const separator = (anchor: string): Allowed => ({ reason: SEPARATOR, anchor });
const arrow = (anchor: string): Allowed => ({ reason: ARROW, anchor });

/** `text-text-ghost` sites, each the padding zeros of a number or pure decoration. */
const GHOST_ALLOWLIST: Allowlist = {
  'app/fiber/channels/[id]/client-page.tsx': [separator('>/</span>')],
  'components/asset/asset-feed.tsx': [separator('> · </span>')],
  'components/asset/detail-shell.tsx': [separator('>/</span>')],
  'components/asset/genetics.tsx': [arrow('aria-hidden="true"')],
  // the mechanism chain's arrow (the amounts' padding zeros are ckb-amount.tsx's)
  'components/dao/dao-board.tsx': [arrow('aria-hidden="true"')],
  'components/scripts/script-family-page.tsx': [separator('>/</span>')],
  'components/transaction/script-group-matrix.tsx': [arrow('aria-hidden="true"')],
  'components/ui/ckb-amount.tsx': [padding('data-ghost')],
  'components/ui/script-byte-map.tsx': [arrow('aria-hidden="true"')],
};

/** `text-<ink>/<n>` sites the gate tolerates: none. */
const ALPHA_INK_ALLOWLIST: Allowlist = {};

const LINKED =
  'a linked highlight: while the reader points at (or pins) one part, the others fade back; their own focus, not a set-back state';
const HOVER =
  'hover feedback on a control or a copyable hash: the pointer’s state, gone when it leaves';
const NO_WORDS = 'fades no word: the element is a mark, an icon or a bar';
const linked = (anchor: string): Allowed => ({ reason: LINKED, anchor });
const hover = (anchor: string): Allowed => ({ reason: HOVER, anchor });
const noWords = (anchor: string): Allowed => ({ reason: NO_WORDS, anchor });

/** `opacity-*` below 1 the gate tolerates. */
const OPACITY_ALLOWLIST: Allowlist = {
  // the witness rows, the dependency rows
  'app/tx/[hash]/client-page.tsx': [linked('lit !== null && !on'), linked('lit !== null && !on')],
  'components/address/address-holdings.tsx': [linked('active !== kind')],
  'components/asset/cell-versions.tsx': [linked('active !== key')],
  'components/asset/object-media.tsx': [hover('hover:opacity-90')],
  // the deposit-window bars: the first day at 90 %, the others at 50 %
  'components/dao/dao-board.tsx': [noWords('bg-gold w-full'), noWords('bg-gold w-full')],
  'components/home/ckbytes-section.tsx': [linked('active !== part')],
  // the menu's chevron icon
  'components/layout/nav-menu.tsx': [noWords('h-3 w-3')],
  // the cell headers, the group columns, the group cards
  'components/transaction/script-group-matrix.tsx': [
    linked('lit?.cells.has(c.id)'),
    linked('dim(on)'),
    linked('dim(on)'),
  ],
  'components/ui/composition-set.tsx': [noWords('<CompositionMark')],
  'components/ui/hex-display.tsx': [hover('copyable && ')],
};

/**
 * Elements that hold words, faded by an `opacity` / `fillOpacity` / `strokeOpacity` attribute or a
 * style `opacity` (the JSX scan), that the gate tolerates. Every one is a pointer's state. A
 * block's plate has no age fade in either orientation (lanes R and P): its marks step back by
 * their fill.
 */
const FADE_ALLOWLIST: Allowlist = {
  // a party node and its label while another party or pair is pointed at or pinned
  'components/fiber/fiber-network-graph.tsx': [linked('lit ? 1 : DIM')],
  // a day's bars and its date while another day is pointed at or pinned
  'components/forks/fork-days-chart.tsx': [linked('active === i')],
  // the spark glyphs, the signed bars with their clip label: while another bucket is pointed at
  'components/home/mini-chart.tsx': [
    linked('bucketOpacity(i, hovered)'),
    linked('bucketOpacity(i, hovered)'),
  ],
  // the window's label while a pointed cellbase's reward link runs through its lane
  'components/pipeline/window-label.tsx': [linked('faded ? 0.15')],
  // a lane and its gene name while another gene or a genotype row is pointed at or pinned
  'components/ui/allele-gel.tsx': [linked('laneOpacity(lane.gene)')],
  // a gene's map, bytes and name while another gene is pointed at
  'components/ui/sequencing-gel.tsx': [linked('lit !== g.name')],
};

const SPARK_GLYPH =
  'no word: a spark glyph (an eighth block) is a bar set in the font, the unfinished bucket’s bar faint';

/**
 * Elements that hold words, painted in an ink under text grade (ghost, quiet, muted) by an
 * attribute or a style, that the gate tolerates.
 */
const FAINT_INK_ALLOWLIST: Allowlist = {
  // the quiet slate's ↔ between a channel's two parties
  'app/fiber/channels/client-page.tsx': [separator('↔')],
  // the quiet slate's ↔ between the two parties of a channel
  'components/fiber/fiber-channel-board.tsx': [separator('↔')],
  'components/home/mini-chart.tsx': [{ reason: SPARK_GLYPH, anchor: 'SPARK_INK[b.state]' }],
  // the quiet slate's aria-hidden → between two holders
  'components/scripts/script-family-board.tsx': [separator('aria-hidden="true"')],
  // the quiet slate's → between a gene's raw bytes and its value
  'components/ui/sequencing-gel.tsx': [separator('col.arrow')],
};

/**
 * A site's own source: its line, and when that line stands in a JSX opening tag written over
 * several lines, the whole tag and the line after it (the element's first child: a separator's
 * glyph). An entry's anchor must stand there; a neighbour's line does not count.
 */
function siteSource(lines: readonly string[], line: number): string {
  const at = line - 1;
  const endsTag = (l: string) => />\s*$/.test(l);
  let start = at;
  while (start > 0 && at - start < 4 && !lines[start].includes('<')) start--;
  if (!lines[start].includes('<')) start = at;
  let end = at;
  while (end < lines.length - 1 && end - at < 4 && !endsTag(lines[end])) end++;
  if (!endsTag(lines[end])) end = at;
  // an opening tag that ends its line has its first child on the next line
  const opensChild = endsTag(lines[end]) && !/(<\/[\w.]*>|\/>)\s*$/.test(lines[end]);
  return lines.slice(start, opensChild ? end + 2 : end + 1).join('\n');
}

interface Site {
  path: string;
  line: number;
  what: string;
  /** The site's own source (siteSource), where its entry's anchor must stand. */
  source: string;
}

/**
 * What the sites of one kind break in its allowlist: a site with no reason (a file not listed, or
 * one more site than reasons), a file with fewer sites than reasons, a reason too short to say why.
 * Each names the path, the line (for the message only) and the class.
 */
function allowlistFailures(sites: readonly Site[], allowlist: Allowlist): string[] {
  const byPath = new Map<string, Site[]>();
  for (const site of sites) byPath.set(site.path, [...(byPath.get(site.path) ?? []), site]);
  const out: string[] = [];
  for (const [path, list] of byPath) {
    const entries = allowlist[path] ?? [];
    list.forEach((site, i) => {
      if (i >= entries.length)
        out.push(
          `${path}:${site.line} ${site.what}: no reason for site ${i + 1} of ${list.length} (${entries.length} listed)`
        );
      else if (!site.source.includes(entries[i].anchor))
        out.push(
          `${path}:${site.line} ${site.what}: its line lacks the anchor "${entries[i].anchor}" of its reason (${entries[i].reason})`
        );
    });
  }
  for (const [path, entries] of Object.entries(allowlist)) {
    const list = byPath.get(path) ?? [];
    if (list.length < entries.length)
      out.push(
        `${path}: ${entries.length} reasons for ${list.length} sites (${list.map((s) => `${s.line} ${s.what}`).join(', ') || 'none'}): drop the reason of each site that went`
      );
    for (const { reason, anchor } of entries) {
      if (reason.trim().length <= 10)
        out.push(`${path}: a reason too short to say why: "${reason}"`);
      if (anchor.trim() === '') out.push(`${path}: an empty anchor names no site`);
    }
  }
  return out;
}

const SCAN_DIRS = ['app', 'components'];
/** The type scale's steps: `text-body/6` would be a size with a line height, not an ink. */
const SIZE_STEPS = new Set(Object.keys(TYPE_SCALE));

const GHOST = /(?<![\w-])(?:[\w-]+:)*text-text-ghost(?![\w-])/g;
/** A colour class with Tailwind's opacity modifier, in any variant: `hover:text-jade/80`. */
const ALPHA_INK = /(?<![\w-])((?:[\w-]+:)*)text-([a-z]+(?:-[a-z]+)*)\/(\d+|\[[^\]\s]+\])(?![\w-])/g;
/** An opacity class in any variant: `opacity-30`, `opacity-[.55]`, `group-hover:opacity-50`. */
const OPACITY = /(?<![\w-])((?:[\w-]+:)*)opacity-(\d+|\[[^\]\s]+\])(?![\w-])/g;

interface Offence {
  line: number;
  what: string;
}

/** The value of an opacity class as a fraction: `30` → 0.3, `[.55]` → 0.55. */
function opacityValue(raw: string): number {
  const value = raw.startsWith('[') ? Number(raw.slice(1, -1)) : Number(raw) / 100;
  if (!Number.isFinite(value)) throw new Error(`opacity-${raw} is not a number`);
  return value;
}

/** Every ghost-ink class, alpha-thinned ink and fading opacity in a source text. */
function inkOffences(code: string): { ghost: Offence[]; alpha: Offence[]; opacity: Offence[] } {
  const ghost: Offence[] = [];
  const alpha: Offence[] = [];
  const opacity: Offence[] = [];
  code.split('\n').forEach((text, i) => {
    for (const m of text.matchAll(GHOST)) ghost.push({ line: i + 1, what: m[0] });
    for (const m of text.matchAll(ALPHA_INK))
      if (!SIZE_STEPS.has(m[2])) alpha.push({ line: i + 1, what: m[0] });
    for (const m of text.matchAll(OPACITY)) {
      const value = opacityValue(m[2]);
      const disabled = m[1].split(':').includes('disabled');
      if (value > 0 && value < 1 && !disabled) opacity.push({ line: i + 1, what: m[0] });
    }
  });
  return { ghost, alpha, opacity };
}

// ---- The JSX scan: opacity and the inks under text grade written as attributes and styles ----

/** SVG elements that draw words. */
const TEXT_TAGS = new Set(['text', 'tspan', 'textPath']);
/** Elements whose children are never painted: a tooltip, a description, the paint servers. */
const UNPAINTED_TAGS = new Set([
  'title',
  'desc',
  'defs',
  'clipPath',
  'mask',
  'pattern',
  'linearGradient',
  'radialGradient',
  'filter',
  'marker',
  'symbol',
  'style',
]);
const FADE_ATTRS = new Set(['opacity', 'fillOpacity', 'strokeOpacity']);
/** The style properties that set the ink of words (`--ink` is read by `text-[color:var(--ink)]`). */
const INK_STYLE_PROPS = new Set(['color', 'fill', '--ink', 'WebkitTextFillColor']);
/** The palette keys under text grade: the ghost, the quiet and the muted ink. */
const FAINT_KEYS = 'text-ghost|ink-quiet|ink-muted';
/**
 * A class that paints words in one of them: the fill of SVG text, the colour of HTML text (the
 * ghost's `text-text-ghost` is the class scan's), and the stroke too on a text element.
 */
const FAINT_FILL_CLASS = new RegExp(
  String.raw`(?<![\w-])(?:[\w-]+:)*(?:fill-text-ghost|(?:fill|text)-ink-(?:quiet|muted))(?![\w-])`
);
const FAINT_STROKE_CLASS = new RegExp(
  String.raw`(?<![\w-])(?:[\w-]+:)*stroke-(?:${FAINT_KEYS})(?![\w-])`
);

/** The modules whose constants the faint-ink check reads by value, by import specifier. */
type TokenModules = Readonly<Record<string, Readonly<Record<string, unknown>>>>;
const TOKEN_MODULES: TokenModules = {
  '@/lib/design-tokens': DESIGN_TOKENS,
  '@/lib/chart-colors': CHART_COLORS,
};

/**
 * A string that names an ink under text grade: the key, its variable, its channels, or the
 * pipeline's `--pipeline-ink-dim` (the muted ink, app/globals.css).
 */
const FAINT_INK = new RegExp(
  String.raw`^(?:${FAINT_KEYS})$|--(?:c|rgb)-(?:${FAINT_KEYS})(?![\w-])|--pipeline-ink-dim(?![\w-])`
);
const isFaintInk = (s: string) => FAINT_INK.test(s);

type Jsx = ts.JsxElement | ts.JsxSelfClosingElement | ts.JsxFragment;
const isJsx = (n: ts.Node): n is Jsx =>
  ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n) || ts.isJsxFragment(n);
const openingOf = (n: ts.JsxElement | ts.JsxSelfClosingElement) =>
  ts.isJsxElement(n) ? n.openingElement : n;
/** A component (`Foo`, `Foo.Bar`), not an SVG or HTML element. */
const isComponent = (tag: string) => /^[A-Z]/.test(tag) || tag.includes('.');

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

const isNothing = (e: ts.Expression) =>
  e.kind === ts.SyntaxKind.NullKeyword ||
  e.kind === ts.SyntaxKind.TrueKeyword ||
  e.kind === ts.SyntaxKind.FalseKeyword ||
  (ts.isIdentifier(e) && e.text === 'undefined');

/** Every faded element and every one painted in an ink under text grade that holds words, in a TSX source. */
function jsxInkOffences(
  code: string,
  modules: TokenModules = TOKEN_MODULES
): { fade: Offence[]; faint: Offence[] } {
  const source = ts.createSourceFile(
    'scan.tsx',
    code,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX
  );
  const text = (n: ts.Node) => n.getText(source).replace(/\s+/g, ' ');
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;
  const tagOf = (n: ts.JsxElement | ts.JsxSelfClosingElement) => text(openingOf(n).tagName);

  // The file's constants by name (every scope: a shadowed name reads all its values), and the
  // names it imports from the token modules.
  const locals = new Map<string, ts.Expression[]>();
  const imports = new Map<string, { module: string; name: string }>();
  const collect = (node: ts.Node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer)
      locals.set(node.name.text, [...(locals.get(node.name.text) ?? []), node.initializer]);
    if (
      ts.isImportDeclaration(node) &&
      ts.isStringLiteral(node.moduleSpecifier) &&
      node.importClause?.namedBindings &&
      ts.isNamedImports(node.importClause.namedBindings)
    )
      for (const el of node.importClause.namedBindings.elements)
        imports.set(el.name.text, {
          module: node.moduleSpecifier.text,
          name: (el.propertyName ?? el.name).text,
        });
    ts.forEachChild(node, collect);
  };
  collect(source);

  // ---- does an element hold words ----
  const exprHoldsWords = (e0: ts.Expression | undefined): boolean => {
    if (!e0) return false;
    const e = unwrap(e0);
    if (isJsx(e)) return holdsWords(e);
    if (ts.isConditionalExpression(e))
      return exprHoldsWords(e.whenTrue) || exprHoldsWords(e.whenFalse);
    if (ts.isBinaryExpression(e)) {
      const op = e.operatorToken.kind;
      if (op === ts.SyntaxKind.AmpersandAmpersandToken) return exprHoldsWords(e.right);
      if (op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken)
        return exprHoldsWords(e.left) || exprHoldsWords(e.right);
    }
    if (isNothing(e)) return false;
    // JSX inside a call (`.map(…)`): what that JSX holds; else a value React writes as text
    const jsx: Jsx[] = [];
    const find = (n: ts.Node): void => {
      if (isJsx(n)) jsx.push(n);
      else ts.forEachChild(n, find);
    };
    find(e);
    return jsx.length ? jsx.some(holdsWords) : true;
  };
  const childHoldsWords = (c: ts.JsxChild): boolean =>
    ts.isJsxText(c)
      ? /\S/.test(c.text)
      : ts.isJsxExpression(c)
        ? exprHoldsWords(c.expression)
        : holdsWords(c);
  function holdsWords(n: Jsx): boolean {
    if (ts.isJsxFragment(n)) return n.children.some(childHoldsWords);
    const tag = tagOf(n);
    if (TEXT_TAGS.has(tag)) return true;
    if (UNPAINTED_TAGS.has(tag)) return false;
    if (ts.isJsxSelfClosingElement(n)) return isComponent(tag);
    return n.children.some(childHoldsWords);
  }

  // ---- can a value fade ----
  const canFade = (e0: ts.Expression): boolean => {
    const e = unwrap(e0);
    if (ts.isNumericLiteral(e) || ts.isStringLiteralLike(e)) {
      const v = Number(e.text);
      return !Number.isFinite(v) || (v > 0 && v < 1);
    }
    if (ts.isConditionalExpression(e)) return canFade(e.whenTrue) || canFade(e.whenFalse);
    if (ts.isBinaryExpression(e)) {
      const op = e.operatorToken.kind;
      if (op === ts.SyntaxKind.AmpersandAmpersandToken) return canFade(e.right);
      if (op === ts.SyntaxKind.BarBarToken || op === ts.SyntaxKind.QuestionQuestionToken)
        return canFade(e.left) || canFade(e.right);
    }
    return !isNothing(e);
  };

  // ---- the strings a value can reach (for the inks under text grade) ----
  const stringsIn = (v: unknown): string[] =>
    typeof v === 'string'
      ? [v]
      : v !== null && typeof v === 'object'
        ? Object.values(v).flatMap(stringsIn)
        : [];
  const runtimeValue = (e: ts.Expression): unknown => {
    if (ts.isIdentifier(e)) {
      const from = imports.get(e.text);
      return from ? modules[from.module]?.[from.name] : undefined;
    }
    if (ts.isPropertyAccessExpression(e) || ts.isElementAccessExpression(e)) {
      const base = runtimeValue(e.expression);
      if (base === null || typeof base !== 'object') return undefined;
      const key = ts.isPropertyAccessExpression(e)
        ? e.name.text
        : ts.isStringLiteralLike(e.argumentExpression)
          ? e.argumentExpression.text
          : null;
      return key === null ? base : (base as Record<string, unknown>)[key];
    }
    return undefined;
  };
  const propertyName = (p: ts.ObjectLiteralElementLike) =>
    p.name && (ts.isIdentifier(p.name) || ts.isStringLiteralLike(p.name)) ? p.name.text : null;
  /** The expressions a name or a member of a file constant stands for. */
  const staticTargets = (e: ts.Expression): ts.Expression[] => {
    if (ts.isIdentifier(e)) return locals.get(e.text) ?? [];
    if (ts.isPropertyAccessExpression(e) || ts.isElementAccessExpression(e)) {
      const key = ts.isPropertyAccessExpression(e)
        ? e.name.text
        : ts.isStringLiteralLike(e.argumentExpression)
          ? e.argumentExpression.text
          : null;
      return staticTargets(unwrap(e.expression)).flatMap((t0) => {
        const t = unwrap(t0);
        if (ts.isArrayLiteralExpression(t)) return key === null ? [...t.elements] : [];
        if (!ts.isObjectLiteralExpression(t)) return [];
        return t.properties.flatMap((p) =>
          ts.isPropertyAssignment(p) && (key === null || propertyName(p) === key)
            ? [p.initializer]
            : []
        );
      });
    }
    return [];
  };
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
      return e.properties.flatMap((p) =>
        ts.isPropertyAssignment(p)
          ? next(p.initializer)
          : ts.isSpreadAssignment(p)
            ? next(p.expression)
            : []
      );
    const value = runtimeValue(e);
    if (value !== undefined) return stringsIn(value);
    return staticTargets(e).flatMap(next);
  };
  const reachesFaint = (e: ts.Expression) => leaves(e).some(isFaintInk);

  /**
   * The values a style gives to `names`, through `?:`, `&&`, `??`, spreads and file constants. A
   * style built by a call (`inkStyle(…)`, alone, spread or held in a constant) is read for the ink
   * only: its arguments may set it.
   */
  const styleValues = (e0: ts.Expression, names: Set<string>, depth = 0): ts.Expression[] => {
    if (depth > 8) return [];
    const e = unwrap(e0);
    const next = (x: ts.Expression) => styleValues(x, names, depth + 1);
    if (ts.isObjectLiteralExpression(e))
      return e.properties.flatMap((p) => {
        if (ts.isSpreadAssignment(p)) return next(p.expression);
        const name = propertyName(p);
        if (name === null || !names.has(name)) return [];
        if (ts.isPropertyAssignment(p)) return [p.initializer];
        return ts.isShorthandPropertyAssignment(p) ? [p.name] : [];
      });
    if (ts.isConditionalExpression(e)) return [...next(e.whenTrue), ...next(e.whenFalse)];
    if (ts.isBinaryExpression(e)) return [...next(e.left), ...next(e.right)];
    if (ts.isIdentifier(e)) return staticTargets(e).flatMap(next);
    if (ts.isCallExpression(e) && names === INK_STYLE_PROPS) return [...e.arguments];
    return [];
  };

  const fade: Offence[] = [];
  const faint: Offence[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tag = tagOf(node);
      const component = isComponent(tag);
      let words: boolean | undefined;
      const holds = () => (words ??= holdsWords(node));
      for (const attr of openingOf(node).attributes.properties) {
        if (!ts.isJsxAttribute(attr) || !attr.initializer) continue;
        const name = text(attr.name);
        const init = attr.initializer;
        const value = ts.isJsxExpression(init)
          ? init.expression
          : ts.isStringLiteral(init)
            ? init
            : undefined;
        if (!value) continue;
        const at = { line: lineOf(attr) };
        if (!component && FADE_ATTRS.has(name) && canFade(value) && holds())
          fade.push({ ...at, what: `<${tag}> ${text(attr)}` });
        if (name === 'style') {
          for (const v of styleValues(value, new Set(['opacity'])))
            if (canFade(v) && holds())
              fade.push({ ...at, what: `<${tag}> style opacity: ${text(v)}` });
          const inks = styleValues(value, INK_STYLE_PROPS);
          if (inks.some(reachesFaint) && holds())
            faint.push({ ...at, what: `<${tag}> ${text(attr)}` });
        }
        const paint =
          !component &&
          (name === 'fill' || name === 'color' || (name === 'stroke' && TEXT_TAGS.has(tag)));
        if (paint && reachesFaint(value) && holds())
          faint.push({ ...at, what: `<${tag}> ${text(attr)}` });
        if (name === 'className') {
          const classes = leaves(value);
          const faintClass =
            classes.some((c) => FAINT_FILL_CLASS.test(c)) ||
            (TEXT_TAGS.has(tag) && classes.some((c) => FAINT_STROKE_CLASS.test(c)));
          if (faintClass && holds()) faint.push({ ...at, what: `<${tag}> ${text(attr)}` });
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return { fade, faint };
}

function sourceFiles(root: string): string[] {
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.(tsx|ts)$/.test(name) && !name.endsWith('.d.ts')) files.push(path);
    }
  };
  for (const dir of SCAN_DIRS) walk(join(root, dir));
  return files;
}

describe('the ink gate finds what it must, and only that', () => {
  it('flags the ghost ink in any variant, never a look-alike class', () => {
    const code = [
      '<span className="text-text-ghost text-meta">at #1</span>',
      "const c = cn(zero ? 'sm:text-text-ghost' : 'text-text', 'hover:text-text-ghost');",
      '<i className="fill-text-ghost border-text-ghost text-text-ghostly bg-text-ghost" />',
    ].join('\n');
    expect(inkOffences(code).ghost).toEqual([
      { line: 1, what: 'text-text-ghost' },
      { line: 2, what: 'sm:text-text-ghost' },
      { line: 2, what: 'hover:text-text-ghost' },
    ]);
  });

  it('flags an ink thinned by an opacity modifier, never a size with a line height', () => {
    const code = [
      '<span className="text-text-dim/60 text-meta">occupied +61</span>',
      "const a = 'hover:text-jade/80 text-aqua/[0.7] text-white/70';",
      "const b = 'text-body/6 text-meta/[18px] bg-jade/10 border-text-dim/60 text-text-dim';",
    ].join('\n');
    expect(inkOffences(code).alpha).toEqual([
      { line: 1, what: 'text-text-dim/60' },
      { line: 2, what: 'hover:text-jade/80' },
      { line: 2, what: 'text-aqua/[0.7]' },
      { line: 2, what: 'text-white/70' },
    ]);
  });

  it('flags an opacity below 1 in any variant but disabled:, never 0, 100 or a look-alike', () => {
    const code = [
      "rowClassName={(tx) => (tx.isCellbase ? 'opacity-[.55]' : '')}",
      '<span className="opacity-[0.78]">{parts.fraction}</span>',
      "const d = cn(dimmed && 'opacity-30', 'group-hover:opacity-50 hover:opacity-90');",
      "const e = 'opacity-0 opacity-100 disabled:opacity-40 bg-opacity-50 transition-opacity';",
    ].join('\n');
    expect(inkOffences(code).opacity).toEqual([
      { line: 1, what: 'opacity-[.55]' },
      { line: 2, what: 'opacity-[0.78]' },
      { line: 3, what: 'opacity-30' },
      { line: 3, what: 'group-hover:opacity-50' },
      { line: 3, what: 'hover:opacity-90' },
    ]);
  });
});

describe('the JSX scan finds faded words and words in an ink under text grade, and only those', () => {
  const fades = (lines: string[]) => jsxInkOffences(lines.join('\n')).fade;
  const faints = (lines: string[], modules?: TokenModules) =>
    jsxInkOffences(lines.join('\n'), modules).faint;

  it('flags SVG text and elements holding words faded by an attribute', () => {
    expect(
      fades([
        '<tspan fillOpacity={0.7}>{parts.fraction}</tspan>;',
        '<text x={1} fill={g.color} fillOpacity=".85">{shortRaw(g.raw)}</text>;',
        '<g opacity={lit && lit !== g.name ? 0.12 : 1}><rect /><text>{g.name}</text></g>;',
        '<text opacity={bucketOpacity(i, hovered)}>{glyphs[i]}</text>;',
        '<g strokeOpacity={0.5}>{cond && <tspan>uncle</tspan>}</g>;',
        '<g opacity={0.4}>{rows.map((r) => <text key={r}>{r}</text>)}</g>;',
        '<g opacity={dim}><ClipMark text={label} /></g>;',
        '<p opacity="0.5">occupied +61</p>;',
      ])
    ).toEqual([
      { line: 1, what: '<tspan> fillOpacity={0.7}' },
      { line: 2, what: '<text> fillOpacity=".85"' },
      { line: 3, what: '<g> opacity={lit && lit !== g.name ? 0.12 : 1}' },
      { line: 4, what: '<text> opacity={bucketOpacity(i, hovered)}' },
      { line: 5, what: '<g> strokeOpacity={0.5}' },
      { line: 6, what: '<g> opacity={0.4}' },
      { line: 7, what: '<g> opacity={dim}' },
      { line: 8, what: '<p> opacity="0.5"' },
    ]);
  });

  it('flags an inline style opacity on what holds words, through ?:, spreads and file constants', () => {
    expect(
      fades([
        '<g data-age={age} style={{ opacity: ageOpacity(age) }}><text>{n}</text></g>;',
        '<div style={{ width: 4, opacity: 0.5 }}>occupied</div>;',
        '<p style={cond ? { opacity: 0.4 } : {}}>{label}</p>;',
        '<span style={{ ...(on ? {} : { opacity: 0.3 }) }}>{hash}</span>;',
        'const faded = { opacity: 0.25 };',
        '<li style={faded}>{name}</li>;',
      ])
    ).toEqual([
      { line: 1, what: '<g> style opacity: ageOpacity(age)' },
      { line: 2, what: '<div> style opacity: 0.5' },
      { line: 3, what: '<p> style opacity: 0.4' },
      { line: 4, what: '<span> style opacity: 0.3' },
      { line: 6, what: '<li> style opacity: 0.25' },
    ]);
  });

  it('passes a shape’s strength, a show or hide, what paints nothing, a component’s own prop', () => {
    expect(
      fades([
        '<rect fillOpacity={0.25} /><circle opacity={share} /><path strokeOpacity=".4" />;',
        '<span style={{ width: `${w}%`, backgroundColor: c, opacity: 0.35 + 0.45 * share }} />;',
        '<g opacity={0.3}>{bars.map((b) => <rect key={b.key} fillOpacity={b.alpha} />)}</g>;',
        '<g opacity={0.55}><title>{reading(day)}</title><rect /></g>;',
        '<g opacity={shown ? 1 : 0}><text>this one</text></g>;',
        '<text opacity={1} fillOpacity="1" strokeOpacity={undefined}>{n}</text>;',
        '<LegendLine stroke={c} width={3} opacity={0.4} />;',
        '<div style={{ opacity: 1 }}>{label}</div>;',
      ])
    ).toEqual([]);
  });

  const tokens: TokenModules = {
    '@/lib/inks': {
      INKS: {
        faint: 'var(--c-text-ghost)',
        slate: 'var(--c-ink-quiet)',
        muted: 'var(--c-ink-muted)',
        dim: 'var(--c-text-dim)',
      },
    },
  };

  it('flags words painted in the ghost by an attribute, a style, a class or a constant', () => {
    expect(
      faints(
        [
          "import { INKS } from '@/lib/inks';",
          "const SPARK = { done: 'var(--c-jade)', todo: 'var(--c-text-ghost)' };",
          '<text fill="var(--c-text-ghost)" fontSize="11">{tickLabel(v)}</text>;',
          "<text fill={color('text-ghost')}>{label}</text>;",
          '<text className="fill-text-ghost text-meta">{label}</text>;',
          "<span style={{ color: on ? tone : 'var(--c-text-ghost)' }}>{ch}</span>;",
          '<text fill={SPARK[b.state]}>{glyph}</text>;',
          '<g fill={INKS.faint}><text>0</text></g>;',
          '<span className="text-[color:var(--ink)]" style={inkStyle(INKS.faint)}>0x00</span>;',
          "<text stroke={fade(color('text-ghost'), 0.5)}>{n}</text>;",
        ],
        tokens
      )
    ).toEqual([
      { line: 3, what: '<text> fill="var(--c-text-ghost)"' },
      { line: 4, what: "<text> fill={color('text-ghost')}" },
      { line: 5, what: '<text> className="fill-text-ghost text-meta"' },
      { line: 6, what: "<span> style={{ color: on ? tone : 'var(--c-text-ghost)' }}" },
      { line: 7, what: '<text> fill={SPARK[b.state]}' },
      { line: 8, what: '<g> fill={INKS.faint}' },
      { line: 9, what: '<span> style={inkStyle(INKS.faint)}' },
      { line: 10, what: "<text> stroke={fade(color('text-ghost'), 0.5)}" },
    ]);
  });

  it('passes the ghost on a shape or a rule, a ground under words, a quieter named ink', () => {
    expect(
      faints(
        [
          "import { INKS } from '@/lib/inks';",
          '<line stroke="var(--c-text-ghost)" /><path className="stroke-text-ghost" />;',
          '<rect fill={color(\'text-ghost\')} className="fill-text-ghost" />;',
          '<g stroke={INKS.faint}><line /><text>{t.label}</text></g>;',
          "<span style={{ backgroundColor: 'var(--c-text-ghost)' }}>{label}</span>;",
          '<text fill={INKS.dim}>{label}</text>;',
          '<span className="text-text-ghost">{parts.ghost}</span>;',
          '<Swatch color={INKS.faint} />;',
        ],
        tokens
      )
    ).toEqual([]);
  });

  // Lane E2 follow-up (R4 is about use, not a name): the quiet and the muted ink are under text
  // grade too (2.31–3.27:1 and 3:1), for marks, rules, ticks, swatches and frames; on words they
  // fail as the ghost does.
  it('flags words painted in the quiet or the muted ink, as the ghost', () => {
    expect(
      faints(
        [
          "import { INKS } from '@/lib/inks';",
          'const QUIET = inkStyle(INKS.slate);',
          '<text x={4} fill="var(--c-ink-quiet)">{t.label}</text>;',
          "<text fill={color('ink-muted')}>uncle</text>;",
          '<text className="fill-ink-quiet text-meta">{b}</text>;',
          '<span className="text-ink-muted">{tick.date}</span>;',
          "<span style={{ color: dim ? 'var(--c-ink-quiet)' : reading.ink }}>{word}</span>;",
          '<span className="text-[color:var(--ink)]" style={QUIET}>0x{offset}</span>;',
          '<text fill={INKS.muted}>{n}</text>;',
        ],
        tokens
      )
    ).toEqual([
      { line: 3, what: '<text> fill="var(--c-ink-quiet)"' },
      { line: 4, what: "<text> fill={color('ink-muted')}" },
      { line: 5, what: '<text> className="fill-ink-quiet text-meta"' },
      { line: 6, what: '<span> className="text-ink-muted"' },
      { line: 7, what: "<span> style={{ color: dim ? 'var(--c-ink-quiet)' : reading.ink }}" },
      { line: 8, what: '<span> style={QUIET}' },
      { line: 9, what: '<text> fill={INKS.muted}' },
    ]);
  });

  it('passes the quiet and the muted ink on a mark, a rule, a swatch or a frame', () => {
    expect(
      faints(
        [
          "import { INKS } from '@/lib/inks';",
          '<line stroke="var(--c-ink-quiet)" /><circle r="1.8" fill="var(--c-ink-quiet)" />;',
          '<rect fill="none" stroke={INKS.slate} strokeDasharray="3 3" />;',
          '<div className="border border-dashed" style={{ borderColor: INKS.slate }}>{head}</div>;',
          "<span style={{ backgroundColor: color('ink-muted') }} />;",
          '<path className="stroke-ink-quiet" /><i className="bg-ink-muted border-ink-quiet" />;',
        ],
        tokens
      )
    ).toEqual([]);
  });
});

describe('the allowlists key a site by its file, its order and its anchor, not its line', () => {
  const SEPARATE = 'decoration: a separator between two parts of a line';
  const PAD = 'padding zeros: the trailing zeros of an amount’s fraction';
  const list: Allowlist = {
    'components/a.tsx': [
      { reason: SEPARATE, anchor: 'aria-hidden' },
      { reason: PAD, anchor: '{parts.ghost}' },
    ],
  };
  const DOT = '<span aria-hidden className="text-text-ghost">·</span>';
  const ZEROS = '<span className="text-text-ghost">{parts.ghost}</span>';
  const site = (line: number, source: string, path = 'components/a.tsx'): Site => ({
    path,
    line,
    what: 'text-text-ghost',
    source,
  });

  it('passes the same sites moved to other lines', () => {
    expect(allowlistFailures([site(10, DOT), site(20, ZEROS)], list)).toEqual([]);
    expect(allowlistFailures([site(14, DOT), site(31, ZEROS)], list)).toEqual([]);
  });

  // The review's item G: n reasons for n sites in order let an allowed site go and a new faded
  // word take its reason; the new site's line lacks the anchor.
  it('fails a traded site: an allowed one gone, a new faded word in its place', () => {
    const word = '<span className="text-text-ghost">{label}</span>';
    expect(allowlistFailures([site(10, DOT), site(20, word)], list)).toEqual([
      `components/a.tsx:20 text-text-ghost: its line lacks the anchor "{parts.ghost}" of its reason (${PAD})`,
    ]);
    // and two allowed sites swapped in order
    expect(allowlistFailures([site(10, ZEROS), site(20, DOT)], list)).toHaveLength(2);
  });

  it('fails an added site, naming its path, line and class', () => {
    expect(allowlistFailures([site(10, DOT), site(20, ZEROS), site(25, ZEROS)], list)).toEqual([
      'components/a.tsx:25 text-text-ghost: no reason for site 3 of 3 (2 listed)',
    ]);
    expect(allowlistFailures([site(7, ZEROS, 'components/b.tsx')], list)).toEqual([
      'components/b.tsx:7 text-text-ghost: no reason for site 1 of 1 (0 listed)',
      'components/a.tsx: 2 reasons for 0 sites (none): drop the reason of each site that went',
    ]);
  });

  it('fails a removed site, naming the file and the sites left', () => {
    expect(allowlistFailures([site(20, DOT)], list)).toEqual([
      'components/a.tsx: 2 reasons for 1 sites (20 text-text-ghost): drop the reason of each site that went',
    ]);
  });

  it('fails a reason too short to say why and an empty anchor', () => {
    expect(
      allowlistFailures([site(1, ZEROS)], {
        'components/a.tsx': [{ reason: 'zeros', anchor: ' ' }],
      })
    ).toEqual([
      'components/a.tsx: a reason too short to say why: "zeros"',
      'components/a.tsx: an empty anchor names no site',
    ]);
  });

  it('reads a site’s own source: its line, or its tag over lines and the first child', () => {
    const lines = [
      '<span className="text-text-dim">{label}</span>',
      '{i > 0 && (',
      '  <span',
      '    aria-hidden="true"',
      '    className="text-text-ghost text-center"',
      '  >',
      '    →',
      '  </span>',
      ')}',
    ];
    expect(siteSource(lines, 5)).toBe(lines.slice(2, 7).join('\n'));
    expect(siteSource(lines, 1)).toBe(lines[0]);
    const oneLine = ['<span className="text-[color:var(--ink)]" style={quiet}>', '  ↔', '</span>'];
    expect(siteSource(oneLine, 1)).toBe(oneLine.slice(0, 2).join('\n'));
  });
});

describe('inks that carry words (R4)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const found = {
    ghost: [] as Site[],
    alpha: [] as Site[],
    opacity: [] as Site[],
    fade: [] as Site[],
    faint: [] as Site[],
  };
  for (const file of files) {
    const path = relative(root, file);
    const code = readFileSync(file, 'utf8');
    const lines = code.split('\n');
    const offences = { ...inkOffences(code), ...jsxInkOffences(code) };
    for (const kind of ['ghost', 'alpha', 'opacity', 'fade', 'faint'] as const)
      for (const o of offences[kind])
        found[kind].push({ path, ...o, source: siteSource(lines, o.line) });
  }
  const rules = [
    ['ghost', GHOST_ALLOWLIST, 'the ghost ink on a word or a figure'],
    ['alpha', ALPHA_INK_ALLOWLIST, 'an ink thinned by an opacity modifier'],
    ['opacity', OPACITY_ALLOWLIST, 'an element faded by opacity'],
    ['fade', FADE_ALLOWLIST, 'words faded by an opacity attribute or style'],
    [
      'faint',
      FAINT_INK_ALLOWLIST,
      'words painted in the ghost, quiet or muted ink by an attribute or style',
    ],
  ] as const;

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(250);
  });

  for (const [kind, allowlist, what] of rules)
    it(`draws no ${what} but the ${kind} allowlist's, each with its reason`, () => {
      expect(allowlistFailures(found[kind], allowlist)).toEqual([]);
    });
});
