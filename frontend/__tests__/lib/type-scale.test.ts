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
