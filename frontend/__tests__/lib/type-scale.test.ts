import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import tailwindConfig from '@/tailwind.config';

/**
 * Gate for the one type scale (hierarchy plan 2026-11-02, rulings R1–R3, S8): the tailwind theme's
 * whole `fontSize` is the eight steps (Tailwind's defaults `text-xs` … `text-9xl` do not exist), and
 * no page code sets a size outside them — no arbitrary `text-[Npx]` class, no default size class and
 * no inline `fontSize` style on an HTML element. Ruling R3 exempts only chart renderers' own text:
 * SVG `<text fontSize=…>` / `<tspan>` attributes and canvas `ctx.font` (the SVG gate,
 * `chart-type-gate.test.ts`, holds SVG text to lib/chart-type.ts).
 *
 * What it reads: every class string in app, components, hooks, lib and src (.ts and .tsx, comments
 * too); every object key `fontSize` / `font-size` (plain, quoted, or computed from a literal) and
 * every `font` shorthand key but `inherit`, outside the style of SVG text; every assignment to
 * `.fontSize` or `.style.font`; every string holding CSS text with a size (`font-size: 12px`,
 * `font: 600 15px/1.5 …`); every `font-size` and `font` shorthand in app/globals.css. A size relative
 * to its parent (`text-[0.5em]`, `text-[80%]`) is an offence too since review-fb-tests finding 7:
 * it was exempt as "not a step of its own", so the 12 px floor was never checked on it; the one site
 * is listed below with the sizes it computes to.
 *
 * What a source gate still cannot see: a size computed at run time into a class name
 * (`text-[${n}px]`: Tailwind would not generate it either), a size set on an element from a module
 * outside the scanned folders, a CSS rule's size through a custom property (`font-size: var(--x)`),
 * the size a relative unit computes to on the page (the allowlist states it by reading the classes
 * of its parents; a parent's change is not seen here), and text a transform scales. (The error
 * pages' fluid `clamp()` type this header once exempted is gone: no `clamp()` size is left in the
 * source or the stylesheet, and a new one is an offence.) The Chrome probes of the readability
 * review measure the rendered sizes.
 */

/** R1, smallest first: name, font size, line height. */
/** Readability R4 (2026-11-18, lane F3) moved the three reading sizes from 11 / 12 / 13 px. */
const R1 = [
  ['meta', '12px', '1.5'],
  ['small', '13px', '1.5'],
  ['body', '14px', '1.6'],
  ['section', '17px', '1.3'],
  ['hero', '22px', '1.15'],
  ['index', '26px', '1.2'],
  ['entity', '30px', '1.15'],
  ['display', '40px', '1.05'],
] as const;

/**
 * Neighbouring steps closer than 1.2 × in R1 as ruled, each with why the pair is not a hierarchy
 * level the eye must tell apart. The test fails if R1 changes and this list goes stale.
 */
const RATIO_EXCEPTIONS: Record<string, string> = {
  'meta→small':
    'the three reading sizes (labels / readouts / prose) are told apart by role and ink, not as levels',
  'small→body':
    'the three reading sizes (labels / readouts / prose) are told apart by role and ink, not as levels',
  'hero→index':
    'index is a page h1 (weight 500) above the hero figures of its own header, not a level of one list',
  'index→entity':
    'never on one page: a page h1 is either index (topic or list page) or entity (entity page)',
};

/** Off-scale sites the gate tolerates, as `path:line` → one-line reason. Empty by ruling R3. */
const ALLOWLIST: Record<string, string> = {};

/**
 * Sizes relative to their parent the gate tolerates (review-fb-tests finding 7), as `path#class` →
 * how many, the sizes they compute to on the page, read from their parents' classes, and the lane
 * that owns the file. Each computes to the 12 px floor or more.
 */
const RELATIVE_ALLOWLIST: Record<string, { count: number; reason: string; lane: string }> = {
  'components/home/ckbytes-section.tsx#text-[0.5em]': {
    count: 1,
    reason:
      'the unit after the supply figure, half its size: the figure is text-index (26 px) below 640 and sm:text-display (40 px) from 640, so " CKB" is 13 and 20 px',
    lane: 'none of the readability lanes (the home page)',
  },
};

/**
 * UI review 2026-11-14, ruling R12 (U-05): the stylesheet is read too — a `font-size` in
 * app/globals.css is a step of the scale or one of these, each a voice or a mark, never running
 * text (tier-2 prose was 14 px there, outside the scale and larger than tier 1's 13 px body).
 */
const CSS_ALLOWLIST: Record<string, string> = {
  '.comment-lg 18px': 'the voice: the 404 verse at 18 px on a phone',
  '.comment-lg 21px': 'the voice: the 404 verse at 21 px from 640 px',
  '.comment-lg::before 16px': 'the 404 verse’s # at 16 px, sized with the verse it leads',
  '.wordmark 15px': 'the wordmark’s name at 15 px: a mark, not a step running text may take',
};

/**
 * R12: every eyebrow (an uppercase label: a tier boundary's, a frame's, a hero figure's) is one
 * size, `text-meta` (ten were `text-small`), as `path:line` → reason for a site that is not.
 */
const EYEBROW_ALLOWLIST: Record<string, string> = {};

const CSS_FILE = 'app/globals.css';
/**
 * In order on a line: a rule's opening (its selector), a font size, a `font` shorthand, a rule's
 * end.
 */
const CSS_TOKEN = /([^{};]*)\{|font-size\s*:\s*([^;]+);|(?<![\w-])font\s*:\s*([^;]+);|\}/g;

/** The size in a `font` shorthand (`600 15px/1.5 …` → `15px`), or null for a keyword (`inherit`). */
function shorthandSize(value: string): string | null {
  for (const token of value.trim().split(/\s+/)) {
    const size = token.split('/')[0];
    if (/^\d*\.?\d+(px|rem|em|%)$/.test(size) || /^(clamp|calc|min|max)\(/.test(size)) return size;
  }
  return null;
}
const STEP_SIZES = new Set<string>(R1.map(([, size]) => size));
/** A string literal holding the class `eyebrow`. */
const EYEBROW_LITERAL = /(['"`])((?:(?!\1).)*?(?<![\w-])eyebrow(?![\w-])(?:(?!\1).)*)\1/g;
const STEP_CLASS =
  /(?<![\w-])(?:[\w-]+:)*text-(meta|small|body|section|hero|index|entity|display)(?![\w-])/g;

/**
 * Every font size declared in a stylesheet that is not a step of the scale, as `selector size`
 * (the rule it is declared in, an `@media` around it skipped): an exception is named by its rule,
 * so it survives another lane's edits above it.
 */
function offScaleCss(css: string): Offence[] {
  const out: Offence[] = [];
  const open: string[] = [];
  css
    .replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' '))
    .split('\n')
    .forEach((text, i) => {
      for (const m of text.matchAll(CSS_TOKEN)) {
        if (m[0] === '}') open.pop();
        else if (m[1] !== undefined) open.push(m[1].trim());
        else {
          const size = m[2] !== undefined ? m[2].trim() : shorthandSize(m[3]);
          if (size === null || STEP_SIZES.has(size)) continue;
          const rule = [...open].reverse().find((s) => !s.startsWith('@')) ?? '(top level)';
          out.push({ line: i + 1, what: `${rule} ${size}` });
        }
      }
    });
  return out;
}

/** The code with its comments blanked, line for line (a comment may name an eyebrow in words). */
function blankComments(code: string): string {
  return code
    .replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' '))
    .replace(/(^|[^:'"`\\])\/\/.*$/gm, '$1');
}

/** Every eyebrow whose class list is not `text-meta` alone among the steps. */
function offSizeEyebrows(code: string): Offence[] {
  const out: Offence[] = [];
  blankComments(code)
    .split('\n')
    .forEach((text, i) => {
      for (const m of text.matchAll(EYEBROW_LITERAL)) {
        const steps = [...m[2].matchAll(STEP_CLASS)].map((s) => s[0]);
        if (steps.length !== 1 || steps[0] !== 'text-meta')
          out.push({ line: i + 1, what: `eyebrow with ${steps.join(' ') || 'no size'}` });
      }
    });
  return out;
}

const SCAN_DIRS = ['app', 'components', 'hooks', 'lib', 'src'];
const SVG_TEXT_TAGS = new Set(['svg', 'g', 'text', 'tspan', 'textPath']);
/**
 * An arbitrary size (`text-[13px]`, `text-[1.5rem]`), a size relative to the parent
 * (`text-[0.5em]`, `text-[80%]`), a computed one (`text-[clamp(…)]`), or a Tailwind default size
 * (`text-xs` … `text-9xl`).
 */
const OFF_SCALE =
  /(?<![\w-])(?:[\w-]+:)*text-(?:\[(?:length:)?(?:\d*\.?\d+(?:px|rem|em|%)|(?:clamp|calc|min|max)\([^\]]*)\]|(?:xs|sm|base|lg|xl|[2-9]xl)(?![\w-]))/g;
/** CSS text that sets a size: `font-size: 12px`, or a `font` shorthand with a size in it. */
const CSS_FONT_SIZE = /font-size\s*:\s*\d|(?<![\w-])font\s*:[^;'"`]*\d/;

interface Offence {
  line: number;
  what: string;
}

/** Every off-scale size class in a source text (comments included: keep them in words). */
function offScaleClasses(code: string): Offence[] {
  const out: Offence[] = [];
  code.split('\n').forEach((text, i) => {
    for (const m of text.matchAll(OFF_SCALE)) out.push({ line: i + 1, what: m[0] });
  });
  return out;
}

/**
 * Every font size set from script on an HTML element: a `fontSize` key in an object literal (a
 * `style={{…}}` or a hoisted style object), an `el.style.fontSize =` assignment, or CSS text with
 * `font-size:`. A `style` on an SVG text element is exempt (R3); so are JSX `fontSize=` attributes
 * and canvas `ctx.font`, which are not object keys, and a `font-size` key whose value is an array or
 * object (a class-group table such as tailwind-merge's), which no CSS size can be.
 */
function inlineFontSizes(file: string, code: string): Offence[] {
  const source = ts.createSourceFile(
    file,
    code,
    ts.ScriptTarget.Latest,
    true,
    file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  const out: Offence[] = [];
  const at = (node: ts.Node) =>
    source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
  // a key plain, quoted, or computed from a literal (`['fontSize']`, review-fb-tests finding 7)
  const keyName = (name: ts.PropertyName) =>
    ts.isIdentifier(name) || ts.isStringLiteral(name)
      ? name.text
      : ts.isComputedPropertyName(name) && ts.isStringLiteralLike(name.expression)
        ? name.expression.text
        : '';
  const onSvgText = (node: ts.Node) => {
    for (let p = node.parent; p; p = p.parent) {
      if (ts.isJsxAttribute(p)) {
        if (p.name.getText(source) !== 'style') return false;
        const element = p.parent.parent;
        return (
          (ts.isJsxOpeningElement(element) || ts.isJsxSelfClosingElement(element)) &&
          SVG_TEXT_TAGS.has(element.tagName.getText(source))
        );
      }
    }
    return false;
  };
  const isTable = (node: ts.Node) =>
    ts.isPropertyAssignment(node) &&
    (ts.isArrayLiteralExpression(node.initializer) ||
      ts.isObjectLiteralExpression(node.initializer));
  /** A `font` shorthand key that sets no size: `inherit` and the other keywords with no digit. */
  const fontKeyword = (node: ts.PropertyAssignment | ts.ShorthandPropertyAssignment) =>
    ts.isPropertyAssignment(node) &&
    ts.isStringLiteralLike(node.initializer) &&
    !/\d/.test(node.initializer.text);
  const visit = (node: ts.Node) => {
    if (
      (ts.isPropertyAssignment(node) || ts.isShorthandPropertyAssignment(node)) &&
      (['fontSize', 'font-size'].includes(keyName(node.name)) ||
        (keyName(node.name) === 'font' && !fontKeyword(node))) &&
      !isTable(node) &&
      !onSvgText(node)
    )
      out.push({ line: at(node), what: node.getText(source).replace(/\s+/g, ' ').slice(0, 60) });
    if (
      ts.isBinaryExpression(node) &&
      node.operatorToken.kind === ts.SyntaxKind.EqualsToken &&
      ts.isPropertyAccessExpression(node.left) &&
      (node.left.name.text === 'fontSize' ||
        // `el.style.font = …`; a canvas's `ctx.font` is not a style (R3)
        (node.left.name.text === 'font' &&
          ts.isPropertyAccessExpression(node.left.expression) &&
          node.left.expression.name.text === 'style'))
    )
      out.push({ line: at(node), what: node.getText(source).slice(0, 60) });
    if (
      (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) &&
      CSS_FONT_SIZE.test(node.text)
    )
      out.push({ line: at(node), what: node.text.slice(0, 60) });
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
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

describe('the type scale (R1)', () => {
  it('makes the eight R1 steps the whole font-size theme, smallest first, with line heights', () => {
    const theme = tailwindConfig.theme as { fontSize?: object; extend: { fontSize?: object } };
    expect(theme.fontSize, 'theme.fontSize').toBeDefined();
    expect(Object.entries(theme.fontSize!)).toEqual(
      R1.map(([name, size, lh]) => [name, [size, lh]])
    );
    // Under `extend` the steps would sit beside Tailwind's defaults, text-xs … text-9xl (S8).
    expect(theme.extend.fontSize, 'theme.extend.fontSize').toBeUndefined();
  });

  it('keeps neighbouring steps at least 1.2 × apart, except the reasoned pairs', () => {
    const under: string[] = [];
    for (let i = 1; i < R1.length; i++) {
      const ratio = parseFloat(R1[i][1]) / parseFloat(R1[i - 1][1]);
      expect(ratio, `${R1[i - 1][0]}→${R1[i][0]}`).toBeGreaterThan(1);
      if (ratio < 1.2) under.push(`${R1[i - 1][0]}→${R1[i][0]}`);
    }
    expect(under).toEqual(Object.keys(RATIO_EXCEPTIONS));
    for (const reason of Object.values(RATIO_EXCEPTIONS)) expect(reason.length).toBeGreaterThan(20);
  });
});

describe('the scale gate finds what it must, and only that', () => {
  it('flags arbitrary px sizes in any variant, never colours or scale steps', () => {
    const code = [
      '<p className="text-[13px] text-[var(--c-text)]">',
      '<b className="text-meta sm:text-[30px] md:hover:text-[length:12.5px]">',
      "const T = 'text-[0.14px] text-[1.5rem] text-section text-[0.85em]';",
    ].join('\n');
    expect(offScaleClasses(code)).toEqual([
      { line: 1, what: 'text-[13px]' },
      { line: 2, what: 'sm:text-[30px]' },
      { line: 2, what: 'md:hover:text-[length:12.5px]' },
      { line: 3, what: 'text-[0.14px]' },
      { line: 3, what: 'text-[1.5rem]' },
      // re-pinned (review-fb-tests finding 7): a size relative to its parent is an offence now,
      // the R4 floor unchecked on it while it was exempt
      { line: 3, what: 'text-[0.85em]' },
    ]);
  });

  it('flags Tailwind default sizes in any variant, never a step or a look-alike class (S8)', () => {
    const code = [
      '<p className="text-xs text-small sm:text-base md:hover:text-2xl">',
      "const s = { sm: 'text-sm/6', lg: 'text-9xl', md: 'text-body' };",
      '<i className="text-smooth text-xs-wide context-xl max-w-sm rounded-xl text-text-dim">',
    ].join('\n');
    expect(offScaleClasses(code)).toEqual([
      { line: 1, what: 'text-xs' },
      { line: 1, what: 'sm:text-base' },
      { line: 1, what: 'md:hover:text-2xl' },
      { line: 2, what: 'text-sm' },
      { line: 2, what: 'text-9xl' },
    ]);
  });

  it('flags a script-set size on an HTML element, not an SVG text attribute or a canvas font', () => {
    const code = [
      'export function A({ s }: { s: number }) {',
      '  const box = { color: "red", fontSize: `${s}px` };',
      '  return (<div style={{ fontSize: "12px" }}><span style={box} />',
      '    <svg><text fontSize={10} style={{ fontSize: 9 }}>x</text><tspan fontSize="8" /></svg>',
      '  </div>);',
      '}',
      'function draw(ctx: CanvasRenderingContext2D, el: HTMLElement) {',
      '  ctx.font = `${12}px sans-serif`;',
      "  el.style.fontSize = '14px';",
      "  el.style.cssText = 'font-size: 15px';",
      '}',
      "const merge = { classGroups: { 'font-size': [{ text: ['meta'] }] } };",
      'const css = { fontSize };',
    ].join('\n');
    expect(inlineFontSizes('a.tsx', code).map((o) => o.line)).toEqual([2, 3, 9, 10, 13]);
  });

  // review-fb-tests finding 7: the routes a size took past the gate.
  it('flags the font shorthand and a computed key on an HTML element, not a canvas font', () => {
    const code = [
      'export function A() {',
      "  const box = { ['fontSize']: 12, [`font-size`]: 13 };",
      "  return <div style={{ font: '12px mono' }}><span style={box} /></div>;",
      '}',
      'function g(el: HTMLElement, ctx: CanvasRenderingContext2D) {',
      "  el.style.font = '12px mono';",
      "  el.style.cssText = 'font: 600 15px/1.5 mono';",
      "  ctx.font = '12px mono';",
      "  const inherits = { font: 'inherit' };",
      '}',
    ].join('\n');
    expect(inlineFontSizes('a.tsx', code).map((o) => o.line)).toEqual([2, 2, 3, 6, 7]);
  });

  it('flags a size relative to its parent: text-[0.5em], text-[80%], in any variant', () => {
    const code = '<span className="text-[0.5em] sm:text-[80%] text-[var(--c-text)] text-meta">';
    expect(offScaleClasses(code)).toEqual([
      { line: 1, what: 'text-[0.5em]' },
      { line: 1, what: 'sm:text-[80%]' },
    ]);
  });

  it('flags a computed size class, clamp or calc', () => {
    expect(
      offScaleClasses('<p className="text-[clamp(1rem,4vw,2rem)] md:text-[calc(1em+2px)]">')
    ).toEqual([
      { line: 1, what: 'text-[clamp(1rem,4vw,2rem)]' },
      { line: 1, what: 'md:text-[calc(1em+2px)]' },
    ]);
  });
});

describe('the stylesheet and eyebrow gates find what they must, and only that (R12)', () => {
  it('flags a declared font size off the scale, never a step', () => {
    const css = [
      '.prose-tier2 p {',
      '  font-size: 15px;',
      '  line-height: 1.7;',
      '}',
      'body { font-size: 13px; }',
      '.x { font-size: 0.85em; } .y { font-size:40px; }',
      '/* a comment: font-size: 9px; */',
      '@media (min-width: 640px) {',
      '  .comment-lg {',
      '    font-size: 21px;',
      '  }',
      '}',
    ].join('\n');
    expect(offScaleCss(css)).toEqual([
      { line: 2, what: '.prose-tier2 p 15px' },
      { line: 6, what: '.x 0.85em' },
      { line: 10, what: '.comment-lg 21px' },
    ]);
  });

  it('reads the size in a font shorthand, never its weight or its line (review-fb-tests 7)', () => {
    const css = [
      '.x { font: 600 15px/1.5 var(--font-mono); }',
      '.y { font: inherit; }',
      '.z { font: 14px var(--font-mono); }',
      '.w {',
      '  font: italic 500 0.9em/2 serif;',
      '}',
    ].join('\n');
    expect(offScaleCss(css)).toEqual([
      { line: 1, what: '.x 15px' },
      { line: 5, what: '.w 0.9em' },
    ]);
  });

  it('flags an eyebrow that is not text-meta, in any quote, never a look-alike', () => {
    const code = [
      '<span className="eyebrow text-text-dim text-small font-mono">x</span>',
      "const a = 'eyebrow text-text-dim text-meta mb-1.5'; const b = `eyebrow ${c}`;",
      '<i className="eyebrows text-small" /><b className="eyebrow sm:text-meta text-meta" />',
      '<h2 className="eyebrow text-text-bright text-body font-mono">K</h2>',
      ' * the "Evidence · for developers" eyebrow — and `className` (a comment, in words)',
      '// an eyebrow "is uppercase" `meta`',
    ].join('\n');
    expect(offSizeEyebrows(code)).toEqual([
      { line: 1, what: 'eyebrow with text-small' },
      { line: 2, what: 'eyebrow with no size' },
      { line: 3, what: 'eyebrow with sm:text-meta text-meta' },
      { line: 4, what: 'eyebrow with text-body' },
    ]);
  });
});

describe('no font size outside the scale in the stylesheet, one size for every eyebrow (R12)', () => {
  const root = join(__dirname, '..', '..');
  // a stylesheet exception is named by its rule and size (`.comment 14px`), not by a line
  const cssOffences = offScaleCss(readFileSync(join(root, CSS_FILE), 'utf8')).map((o) => o.what);
  const eyebrows = sourceFiles(root).flatMap((file) =>
    offSizeEyebrows(readFileSync(file, 'utf8')).map(
      (o) => `${relative(root, file)}:${o.line} ${o.what}`
    )
  );
  const key = (offence: string) => offence.split(' ')[0];

  it('declares no font size off the scale in globals.css, but the voice and the wordmark', () => {
    expect(cssOffences.filter((o) => !(o in CSS_ALLOWLIST))).toEqual([]);
  });

  it('sets tier-2 prose at the body step, 14 px, on its 1.7 line and its measure', () => {
    const css = readFileSync(join(root, CSS_FILE), 'utf8');
    const p = css.match(/\.prose-tier2 p\s*\{([^}]*)\}/)![1];
    expect(p).toContain('font-size: 14px;');
    expect(p).toContain('line-height: 1.7;');
    expect(p).toContain('max-width: 72ch;');
  });

  it('sets every eyebrow at text-meta', () => {
    expect(eyebrows.filter((o) => !(key(o) in EYEBROW_ALLOWLIST))).toEqual([]);
  });

  it('keeps the stylesheet and eyebrow allowlists reasoned and current', () => {
    // pinned: an entry more is a ruling, not a line added here
    expect(Object.keys(CSS_ALLOWLIST)).toHaveLength(4);
    expect(Object.keys(EYEBROW_ALLOWLIST)).toHaveLength(0);
    for (const [allowlist, sites] of [
      [CSS_ALLOWLIST, cssOffences],
      [EYEBROW_ALLOWLIST, eyebrows.map(key)],
    ] as const)
      for (const [site, reason] of Object.entries(allowlist)) {
        expect(reason.trim().length, site).toBeGreaterThan(10);
        expect(sites, site).toContain(site);
      }
  });
});

describe('no font size outside the scale (R2, R3)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const found = files.flatMap((file) => {
    const code = readFileSync(file, 'utf8');
    const path = relative(root, file);
    return [...offScaleClasses(code), ...inlineFontSizes(file, code)].map((o) => ({
      site: `${path}:${o.line} ${o.what}`,
      relative: `${path}#${o.what}`,
    }));
  });
  const offences = found.map((o) => o.site);
  const key = (offence: string) => offence.split(' ')[0];
  /** How often each `path#class` occurs, so a relative size listed once covers one site only. */
  const counts = new Map<string, number>();
  for (const o of found) counts.set(o.relative, (counts.get(o.relative) ?? 0) + 1);
  const listed = (o: (typeof found)[number]) =>
    key(o.site) in ALLOWLIST ||
    (o.relative in RELATIVE_ALLOWLIST &&
      counts.get(o.relative) === RELATIVE_ALLOWLIST[o.relative].count);

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(300);
  });

  it('finds no off-scale class (text-[Npx], text-[Nem], text-xs …) and no inline fontSize on an HTML element', () => {
    expect(found.filter((o) => !listed(o)).map((o) => o.site)).toEqual([]);
  });

  it('keeps the allowlists reasoned, current and their length pinned', () => {
    expect(Object.keys(ALLOWLIST)).toHaveLength(0);
    expect(Object.keys(RELATIVE_ALLOWLIST)).toHaveLength(1);
    for (const [site, reason] of Object.entries(ALLOWLIST)) {
      expect(reason.trim().length, site).toBeGreaterThan(10);
      expect(offences.map(key), site).toContain(site);
    }
    for (const [site, { reason }] of Object.entries(RELATIVE_ALLOWLIST)) {
      expect(reason.trim().length, site).toBeGreaterThan(10);
      expect(
        found.map((o) => o.relative),
        site
      ).toContain(site);
    }
  });
});
