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
 * SVG `<text fontSize=…>` / `<tspan>` attributes and canvas `ctx.font`. Sizes relative to their
 * parent (`text-[0.85em]` decimals and units) and the error pages' fluid `clamp()` artwork type are
 * not steps of their own and are not gated; the measurement script reports them as `offScaleRel`.
 */

/** R1, smallest first: name, font size, line height. */
const R1 = [
  ['meta', '11px', '1.5'],
  ['small', '12px', '1.5'],
  ['body', '13px', '1.6'],
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
 * UI review 2026-11-14, ruling R12 (U-05): the stylesheet is read too — a `font-size` in
 * app/globals.css is a step of the scale or one of these, each a voice or a mark, never running
 * text (tier-2 prose was 14 px there, outside the scale and larger than tier 1's 13 px body).
 */
const CSS_ALLOWLIST: Record<string, string> = {
  '.comment 14px': 'the voice (Courier Prime): a comment line at 14 px',
  '.comment-lg 18px': 'the voice: the 404 verse at 18 px on a phone',
  '.comment-lg 21px': 'the voice: the 404 verse at 21 px from 640 px',
  '.comment-lg::before 16px': 'the 404 verse’s # at 16 px, sized with the verse it leads',
  '.wordmark 15px': 'the wordmark’s name at 15 px: a mark, not a step running text may take',
};

/**
 * R12: every eyebrow (an uppercase label: a tier boundary's, a frame's, a hero figure's) is one
 * size, `text-meta` (ten were `text-small`), as `path:line` → reason for a site that is not.
 */
const EYEBROW_ALLOWLIST: Record<string, string> = {
  'components/layout/navbar-dropdown-styles.ts:14':
    'left for lane B4, which owns components/layout/**; listed in the B2 hand-back',
};

const CSS_FILE = 'app/globals.css';
/** In order on a line: a rule's opening (its selector), a font size, a rule's end. */
const CSS_TOKEN = /([^{};]*)\{|font-size\s*:\s*([^;]+);|\}/g;
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
        else if (m[2] === undefined) open.push(m[1].trim());
        else if (!STEP_SIZES.has(m[2].trim())) {
          const rule = [...open].reverse().find((s) => !s.startsWith('@')) ?? '(top level)';
          out.push({ line: i + 1, what: `${rule} ${m[2].trim()}` });
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
/** An arbitrary px/rem size (`text-[13px]`) or a Tailwind default size (`text-xs` … `text-9xl`). */
const OFF_SCALE =
  /(?<![\w-])(?:[\w-]+:)*text-(?:\[(?:length:)?\d*\.?\d+(?:px|rem)\]|(?:xs|sm|base|lg|xl|[2-9]xl)(?![\w-]))/g;
const CSS_FONT_SIZE = /font-size\s*:\s*\d/;

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
  const keyName = (name: ts.PropertyName) =>
    ts.isIdentifier(name) || ts.isStringLiteral(name) ? name.text : '';
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
  const visit = (node: ts.Node) => {
    if (
      (ts.isPropertyAssignment(node) || ts.isShorthandPropertyAssignment(node)) &&
      ['fontSize', 'font-size'].includes(keyName(node.name)) &&
      !isTable(node) &&
      !onSvgText(node)
    )
      out.push({ line: at(node), what: node.getText(source).replace(/\s+/g, ' ').slice(0, 60) });
    if (
      ts.isBinaryExpression(node) &&
      node.operatorToken.kind === ts.SyntaxKind.EqualsToken &&
      ts.isPropertyAccessExpression(node.left) &&
      node.left.name.text === 'fontSize'
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
});

describe('the stylesheet and eyebrow gates find what they must, and only that (R12)', () => {
  it('flags a declared font size off the scale, never a step', () => {
    const css = [
      '.prose-tier2 p {',
      '  font-size: 14px;',
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
      { line: 2, what: '.prose-tier2 p 14px' },
      { line: 6, what: '.x 0.85em' },
      { line: 10, what: '.comment-lg 21px' },
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

  it('sets tier-2 prose at the body step, 13 px, on its 1.7 line and its measure', () => {
    const css = readFileSync(join(root, CSS_FILE), 'utf8');
    const p = css.match(/\.prose-tier2 p\s*\{([^}]*)\}/)![1];
    expect(p).toContain('font-size: 13px;');
    expect(p).toContain('line-height: 1.7;');
    expect(p).toContain('max-width: 64ch;');
  });

  it('sets every eyebrow at text-meta', () => {
    expect(eyebrows.filter((o) => !(key(o) in EYEBROW_ALLOWLIST))).toEqual([]);
  });

  it('keeps the stylesheet and eyebrow allowlists reasoned and current', () => {
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
  const offences = files.flatMap((file) => {
    const code = readFileSync(file, 'utf8');
    const path = relative(root, file);
    return [...offScaleClasses(code), ...inlineFontSizes(file, code)].map(
      (o) => `${path}:${o.line} ${o.what}`
    );
  });
  const key = (offence: string) => offence.split(' ')[0];

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(300);
  });

  it('finds no off-scale class (text-[Npx], text-xs …) and no inline fontSize on an HTML element', () => {
    expect(offences.filter((o) => !(key(o) in ALLOWLIST))).toEqual([]);
  });

  it('keeps the allowlist reasoned and current', () => {
    for (const [site, reason] of Object.entries(ALLOWLIST)) {
      expect(reason.trim().length, site).toBeGreaterThan(10);
      expect(offences.map(key), site).toContain(site);
    }
  });
});
