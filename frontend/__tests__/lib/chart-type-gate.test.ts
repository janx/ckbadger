import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { SVG_TEXT, SVG_TEXT_FLOOR } from '@/lib/chart-type';

/**
 * Gate for SVG text sizes (readability plan 2026-11-18, lane F3): the type gate
 * (`type-scale.test.ts`) keeps HTML on the scale and exempts SVG text (ruling R3); this one holds
 * SVG text to its own table, `lib/chart-type.ts`, which reads the scale. Every `fontSize` set as a
 * JSX attribute (on a `<text>`, a `<tspan>` or the `<g>` around them) or as a `style` key on an
 * SVG text element takes its size from that module: its expression names an import of
 * `@/lib/chart-type` (directly, or through a `const` of the same file that does) and holds no
 * number of its own. A literal (`fontSize="10"`), an arithmetic on a role (`SVG_TEXT.label + 1`) or
 * a size from anywhere else is an offence. Canvas text (`ctx.font`) is not SVG and not gated. When
 * it was written it found 109 sizes of their own in 15 files; lane F3 converted them one family a
 * commit, the count of each file still to convert pinned in the gate until none was left.
 */

const CHART_TYPE = '@/lib/chart-type';
const SCAN_DIRS = ['app', 'components', 'hooks', 'lib', 'src'];
const SVG_TEXT_TAGS = new Set(['svg', 'g', 'text', 'tspan', 'textPath']);

/** Sites that keep a size of their own, as `path#expression` → how many and why. */
const ALLOWLIST: Record<string, { count: number; reason: string }> = {
  'components/home/mini-chart.tsx#fontSize={f1(size)}': {
    count: 1,
    reason:
      'the sparkline: eighth-block glyphs (▁…█) drawn as bars, one glyph a slot wide, so under 12 px where the slots are narrow (a phone, a long range); they are marks, not words to read, and the bucket values are in the readout',
  },
};

interface Offence {
  line: number;
  what: string;
}

/** The offences an allowlist does not cover, as `path:line what`. */
function unlisted(
  offences: readonly { site: string; key: string }[],
  allowlist: Record<string, { count: number; reason: string }>
): string[] {
  return offences.filter((o) => !(o.key in allowlist)).map((o) => o.site);
}

/** Every `fontSize` in a TSX source that does not come from lib/chart-type.ts. */
function offSvgSizes(file: string, code: string): Offence[] {
  const source = ts.createSourceFile(file, code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const imported = new Set<string>();
  for (const statement of source.statements) {
    if (
      !ts.isImportDeclaration(statement) ||
      !ts.isStringLiteral(statement.moduleSpecifier) ||
      statement.moduleSpecifier.text !== CHART_TYPE
    )
      continue;
    const bindings = statement.importClause?.namedBindings;
    if (bindings && ts.isNamedImports(bindings))
      for (const element of bindings.elements) imported.add(element.name.text);
    if (bindings && ts.isNamespaceImport(bindings)) imported.add(bindings.name.text);
  }
  // A const of this file whose initializer is a size from the table may stand for it.
  const consts = new Map<string, ts.Expression>();
  const collect = (node: ts.Node) => {
    if (
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.initializer &&
      ts.isVariableDeclarationList(node.parent) &&
      node.parent.flags & ts.NodeFlags.Const
    )
      consts.set(node.name.text, node.initializer);
    ts.forEachChild(node, collect);
  };
  collect(source);
  /**
   * A size from the table: a role read off an import (`SVG_TEXT.tick`, `SVG_TEXT[role]`), a choice
   * between such sizes (`compact ? SVG_TEXT.tick : SVG_TEXT.label`), or a const of this file
   * holding one. Arithmetic, a call or a literal is a size of its own.
   */
  const fromTable = (expression: ts.Expression, depth = 0): boolean => {
    if (ts.isParenthesizedExpression(expression)) return fromTable(expression.expression, depth);
    if (ts.isConditionalExpression(expression))
      return fromTable(expression.whenTrue, depth) && fromTable(expression.whenFalse, depth);
    let root: ts.Expression = expression;
    while (ts.isPropertyAccessExpression(root) || ts.isElementAccessExpression(root))
      root = root.expression;
    if (!ts.isIdentifier(root)) return false;
    if (imported.has(root.text)) return true;
    return (
      root === expression &&
      depth === 0 &&
      consts.has(root.text) &&
      fromTable(consts.get(root.text)!, depth + 1)
    );
  };
  const out: Offence[] = [];
  const at = (node: ts.Node) =>
    source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
  const judge = (node: ts.Node, value: ts.Expression | undefined) => {
    if (value === undefined || !fromTable(value))
      out.push({ line: at(node), what: node.getText(source).replace(/\s+/g, ' ').slice(0, 60) });
  };
  const visit = (node: ts.Node) => {
    if (ts.isJsxAttribute(node) && node.name.getText(source) === 'fontSize') {
      const init = node.initializer;
      judge(node, init && ts.isJsxExpression(init) ? init.expression : init);
    }
    if (
      ts.isJsxAttribute(node) &&
      node.name.getText(source) === 'style' &&
      SVG_TEXT_TAGS.has(node.parent.parent.tagName.getText(source)) &&
      node.initializer &&
      ts.isJsxExpression(node.initializer) &&
      node.initializer.expression &&
      ts.isObjectLiteralExpression(node.initializer.expression)
    )
      for (const property of node.initializer.expression.properties)
        if (
          ts.isPropertyAssignment(property) &&
          ['fontSize', 'font-size'].includes(property.name.getText(source).replace(/['"]/g, ''))
        )
          judge(property, property.initializer);
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
      else if (name.endsWith('.tsx')) files.push(path);
    }
  };
  for (const dir of SCAN_DIRS) walk(join(root, dir));
  return files;
}

describe('the SVG size gate finds what it must, and only that', () => {
  it('flags a literal, an off-table import and arithmetic on a role; passes the table', () => {
    const code = [
      "import { SVG_TEXT } from '@/lib/chart-type';",
      "import { NAME_PX } from '@/lib/elsewhere';",
      'const LABEL = SVG_TEXT.label;',
      'const BIG = 14;',
      'export function A({ compact, h }: { compact: boolean; h: number }) {',
      '  const px = h >= 40 ? SVG_TEXT.reading : SVG_TEXT.label;',
      '  return (<svg><g fontSize={10}>',
      '    <text fontSize="11">a</text><text fontSize={NAME_PX}>b</text>',
      '    <text fontSize={SVG_TEXT.tick}>c</text><tspan fontSize={compact ? SVG_TEXT.tick : SVG_TEXT.label} />',
      '    <text fontSize={LABEL}>d</text><text fontSize={px}>e</text><text fontSize={BIG}>f</text>',
      '    <text fontSize={SVG_TEXT.label + 1}>g</text><text style={{ fontSize: 9 }}>h</text>',
      '    <text style={{ fontSize: SVG_TEXT.note }}>i</text>',
      '  </g></svg>);',
      '}',
    ].join('\n');
    expect(offSvgSizes('a.tsx', code)).toEqual([
      { line: 7, what: 'fontSize={10}' },
      { line: 8, what: 'fontSize="11"' },
      { line: 8, what: 'fontSize={NAME_PX}' },
      { line: 10, what: 'fontSize={BIG}' },
      { line: 11, what: 'fontSize={SVG_TEXT.label + 1}' },
      { line: 11, what: 'fontSize: 9' },
    ]);
  });

  // review-fb-tests finding 7, review-fa-tests finding 5: the routes a size took past the gate.
  it('reads a name in its own scope: a const of one function, a parameter shadowing a role', () => {
    const code = [
      "import { SVG_TEXT } from '@/lib/chart-type';",
      'const LABEL = SVG_TEXT.label;',
      'function A() {',
      '  const px = 10;',
      '  return <text fontSize={px}>a</text>;',
      '}',
      'function B() {',
      '  const px = SVG_TEXT.label;',
      '  return <text fontSize={px}>b</text>;',
      '}',
      'function C(LABEL: number) {',
      '  return <text fontSize={LABEL}>c</text>;',
      '}',
      'function D({ SVG_TEXT }: { SVG_TEXT: { tick: number } }) {',
      '  return <text fontSize={SVG_TEXT.tick}>d</text>;',
      '}',
      'const E = () => <text fontSize={LABEL}>e</text>;',
    ].join('\n');
    expect(offSvgSizes('a.tsx', code)).toEqual([
      { line: 5, what: 'fontSize={px}' },
      { line: 12, what: 'fontSize={LABEL}' },
      { line: 15, what: 'fontSize={SVG_TEXT.tick}' },
    ]);
  });

  it('reads the font shorthand, a computed key, a style held in a name, a spread', () => {
    const code = [
      "import { SVG_TEXT } from '@/lib/chart-type';",
      'const ok = { fontSize: SVG_TEXT.tick };',
      "const bad = { fontSize: 10, fill: 'red' };",
      'export function A(props: object, s: object) {',
      '  const fontSize = 11;',
      '  return (<svg>',
      "    <text style={{ font: '10px mono' }}>a</text>",
      '    <text font="10px mono">b</text>',
      "    <text style={{ ['fontSize']: 10 }}>c</text>",
      '    <text style={ok}>d</text><text style={bad}>e</text>',
      '    <text style={s}>f</text>',
      '    <text {...props}>g</text>',
      '    <text style={{ ...bad }}>h</text>',
      '    <text style={{ fontSize }}>i</text>',
      '    <g {...props}><text fontSize={SVG_TEXT.tick}>j</text></g>',
      '    <svg {...props}><path d="M0 0" /></svg>',
      '  </svg>);',
      '}',
    ].join('\n');
    expect(offSvgSizes('a.tsx', code)).toEqual([
      { line: 7, what: "font: '10px mono'" },
      { line: 8, what: 'font="10px mono"' },
      { line: 9, what: "['fontSize']: 10" },
      { line: 10, what: 'fontSize: 10 (style={bad})' },
      { line: 11, what: 'style={s}' },
      { line: 12, what: '{...props}' },
      { line: 13, what: 'fontSize: 10 (...bad)' },
      { line: 14, what: 'fontSize' },
      { line: 15, what: '{...props}' },
    ]);
  });

  it('reads a .ts source: a size set as an attribute, and SVG markup in a string', () => {
    const code = [
      "import { SVG_TEXT } from '@/lib/chart-type';",
      'export function f(el: Element) {',
      "  el.setAttribute('font-size', '10');",
      "  el.setAttribute('font-size', String(SVG_TEXT.tick));",
      "  el.setAttribute('font', '10px mono');",
      '  return \'<svg><text font-size="10">x</text></svg>\';',
      '}',
    ].join('\n');
    expect(offSvgSizes('a.ts', code)).toEqual([
      { line: 3, what: "el.setAttribute('font-size', '10')" },
      { line: 5, what: "el.setAttribute('font', '10px mono')" },
      { line: 6, what: 'font-size="10"' },
    ]);
  });

  it('counts an allowlisted expression: a second site of it is an offence', () => {
    const at = (line: number) => ({
      site: `x.tsx:${line} fontSize={f(s)}`,
      key: 'x.tsx#fontSize={f(s)}',
    });
    const list = { 'x.tsx#fontSize={f(s)}': { count: 1, reason: 'one glyph bar, read elsewhere' } };
    expect(unlisted([at(3)], list)).toEqual([]);
    expect(unlisted([at(3), at(9)], list)).toEqual([
      'x.tsx:3 fontSize={f(s)}',
      'x.tsx:9 fontSize={f(s)}',
    ]);
  });
});

describe('the floor (readability R4): no SVG text under 12 px', () => {
  it('holds every role of the table at or above the floor, the meta step', () => {
    expect(SVG_TEXT_FLOOR).toBe(12);
    expect(
      Object.entries(SVG_TEXT).filter(([, px]) => px < SVG_TEXT_FLOOR),
      'roles under the floor'
    ).toEqual([]);
  });
});

describe('no SVG text size outside lib/chart-type.ts', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const offences = files.flatMap((file) =>
    offSvgSizes(file, readFileSync(file, 'utf8')).map((o) => ({
      site: `${relative(root, file)}:${o.line} ${o.what}`,
      key: `${relative(root, file)}#${o.what}`,
    }))
  );

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(250);
  });

  it('finds no SVG fontSize that does not come from lib/chart-type.ts', () => {
    expect(unlisted(offences, ALLOWLIST)).toEqual([]);
  });

  it('keeps the allowlist reasoned and current', () => {
    // one site keeps a size of its own; a new one is a ruling, not a line added here
    expect(Object.keys(ALLOWLIST)).toHaveLength(1);
    for (const [key, { reason }] of Object.entries(ALLOWLIST)) {
      expect(reason.trim().length, key).toBeGreaterThan(10);
      expect(
        offences.map((o) => o.key),
        key
      ).toContain(key);
    }
  });
});
