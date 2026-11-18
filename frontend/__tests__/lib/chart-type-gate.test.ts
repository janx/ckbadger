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

/** Sites that keep a size of their own, as `path#expression` → why. */
const ALLOWLIST: Record<string, string> = {
  'components/home/mini-chart.tsx#fontSize={f1(size)}':
    'the sparkline: eighth-block glyphs (▁…█) drawn as bars, one glyph a slot wide, so under 12 px where the slots are narrow (a phone, a long range); they are marks, not words to read, and the bucket values are in the readout',
};

interface Offence {
  line: number;
  what: string;
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
    expect(offences.filter((o) => !(o.key in ALLOWLIST)).map((o) => o.site)).toEqual([]);
  });

  it('keeps the allowlist reasoned and current', () => {
    for (const [key, reason] of Object.entries(ALLOWLIST)) {
      expect(reason.trim().length, key).toBeGreaterThan(10);
      expect(
        offences.map((o) => o.key),
        key
      ).toContain(key);
    }
  });
});
