import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import { SVG_TEXT, SVG_TEXT_FLOOR } from '@/lib/chart-type';

/**
 * Gate for SVG text sizes (readability plan 2026-11-18, lane F3): the type gate
 * (`type-scale.test.ts`) keeps HTML on the scale and exempts SVG text (ruling R3); this one holds
 * SVG text to its own table, `lib/chart-type.ts`, which reads the scale. Every size given to SVG
 * text takes it from that module: its expression names an import of `@/lib/chart-type` (directly,
 * or through a `const` that does, found in the scope the name is read in) and holds no number of
 * its own. A literal (`fontSize="10"`), an arithmetic on a role (`SVG_TEXT.label + 1`), a parameter
 * or a size from anywhere else is an offence. Canvas text (`ctx.font`) is not SVG and not gated.
 * When it was written it found 109 sizes of their own in 15 files; lane F3 converted them one
 * family a commit, the count of each file still to convert pinned in the gate until none was left.
 *
 * What it reads, in every .ts and .tsx source of the app: a `fontSize` (or `font-size`) JSX
 * attribute on any element; a `font` attribute (the SVG shorthand, a size of its own whatever it
 * holds); the `style` of an SVG text element (`svg`, `g`, `text`, `tspan`, `textPath`), an object
 * written there or a `const` one it names, its keys plain, quoted or computed from a literal, a
 * shorthand `{ fontSize }`, a spread of a `const` object; a spread of props on `text`, `tspan`,
 * `textPath`, or on an `svg` or `g` with SVG text inside; `setAttribute('font-size' | 'font', …)`;
 * SVG markup in a string (`font-size="…"`). Each name is resolved in its own scope (review-fb-tests
 * finding 7: the consts of a file were once one map by name, so a `const px = 10` of one function
 * passed as another's `const px = SVG_TEXT.label`). What it cannot read is an offence, not a pass:
 * a style or a spread whose object it cannot find, a computed key it cannot name.
 *
 * What a source gate still cannot see: a size that reaches SVG text from a stylesheet (the type
 * gate reads app/globals.css against the scale, not against this table); text drawn smaller than
 * its declared size by a viewBox wider than the box it is drawn in, or by a transform (the figure
 * tests measure the drawn boxes, `__tests__/utils/svg-text-boxes.ts`, and the Chrome probes the
 * rendered size); a size passed to a component as a prop and set from that prop inside it, which
 * is read only where it is set, as a parameter; a module that re-exports this table under another
 * path (read as a size from anywhere else, an offence, never a pass).
 */

const CHART_TYPE = '@/lib/chart-type';
const SCAN_DIRS = ['app', 'components', 'hooks', 'lib', 'src'];
const SVG_TEXT_TAGS = new Set(['svg', 'g', 'text', 'tspan', 'textPath']);
/** The SVG elements whose own text a spread of props could size. */
const SVG_WORD_TAGS = new Set(['text', 'tspan', 'textPath']);
const SIZE_KEYS = new Set(['fontSize', 'font-size']);
/** SVG markup that sizes text, written in a string. */
const MARKUP_SIZE = /\bfont(?:-size)?\s*=\s*["'{\\]/;

/** Sites that keep a size of their own, as `path#expression` → how many and why. */
const ALLOWLIST: Record<string, { count: number; reason: string }> = {
  'components/home/mini-chart.tsx#fontSize={f1(size)}': {
    count: 1,
    reason:
      'the sparkline: eighth-block glyphs (▁…█) drawn as bars, one glyph a slot wide, so under 12 px where the slots are narrow (a phone, a long range); they are marks, not words to read, and the bucket values are in the readout',
  },
};

/**
 * Spreads of props on SVG text the gate cannot read (a binder from another module), each read by
 * hand, as `path#spread` → how many, why it carries no size, and the lane that owns the file.
 */
const UNREAD_SPREADS: Record<string, { count: number; reason: string; lane: string }> = {
  "components/fiber/fiber-network-graph.tsx#{...handlers({ kind: 'node', id: node.id })}": {
    count: 1,
    reason:
      'the pinned readout’s binder and a cursor class: hooks/usePinnedReadout.ts PinnedTargetProps is tabIndex, role, aria-pressed and pointer, focus and key handlers, no size',
    lane: 'none of the readability lanes (the Fiber network graph)',
  },
  'components/ui/sequencing-gel.tsx#{...bind(g.name)}': {
    count: 1,
    reason:
      'the pinned readout’s binder on a gene’s group: hooks/usePinnedReadout.ts PinnedTargetProps is tabIndex, role, aria-pressed and pointer, focus and key handlers, no size',
    lane: 'the figures lane (the gels)',
  },
};

interface Offence {
  line: number;
  what: string;
}

/**
 * The offences an allowlist does not cover, as `path:line what`: every site of a key that is not
 * listed, and every site of a listed key that occurs more often than its count (a second site of
 * an allowlisted expression is no exemption).
 */
function unlisted(
  offences: readonly { site: string; key: string }[],
  allowlist: Record<string, { count: number; reason: string }>
): string[] {
  const counts = new Map<string, number>();
  for (const o of offences) counts.set(o.key, (counts.get(o.key) ?? 0) + 1);
  return offences
    .filter((o) => !(o.key in allowlist) || counts.get(o.key) !== allowlist[o.key].count)
    .map((o) => o.site);
}

/** What a name is where it is read: an import of the table, a `const`, or anything else. */
type Binding =
  | { kind: 'table' }
  | { kind: 'const'; initializer: ts.Expression; path: (string | number)[] }
  | { kind: 'function'; node: ts.FunctionDeclaration }
  | { kind: 'other' };

/** The names a binding pattern declares, each with its path inside the value it destructures. */
function boundNames(
  name: ts.BindingName,
  path: (string | number)[] = []
): { name: string; path: (string | number)[] }[] {
  if (ts.isIdentifier(name)) return [{ name: name.text, path }];
  return name.elements.flatMap((element, i) => {
    if (ts.isOmittedExpression(element)) return [];
    const key = ts.isObjectBindingPattern(name)
      ? (element.propertyName ?? element.name).getText()
      : i;
    return boundNames(element.name, [...path, key]);
  });
}

/** Every `fontSize` and SVG text size in a .ts or .tsx source that does not come from the table. */
function offSvgSizes(file: string, code: string): Offence[] {
  const source = ts.createSourceFile(
    file,
    code,
    ts.ScriptTarget.Latest,
    true,
    file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );

  /** The declarations a scope makes itself (not those of a scope inside it), by name. */
  const declaredIn = (scope: ts.Node, name: string): Binding | null => {
    if (ts.isFunctionLike(scope)) {
      for (const p of scope.parameters)
        if (boundNames(p.name).some((b) => b.name === name)) return { kind: 'other' };
      if (
        (ts.isFunctionExpression(scope) || ts.isFunctionDeclaration(scope)) &&
        scope.name?.text === name
      )
        return { kind: 'other' };
      return null;
    }
    if (ts.isCatchClause(scope) && scope.variableDeclaration)
      return boundNames(scope.variableDeclaration.name).some((b) => b.name === name)
        ? { kind: 'other' }
        : null;
    if (
      (ts.isForStatement(scope) || ts.isForOfStatement(scope) || ts.isForInStatement(scope)) &&
      scope.initializer &&
      ts.isVariableDeclarationList(scope.initializer)
    )
      for (const d of scope.initializer.declarations)
        if (boundNames(d.name).some((b) => b.name === name)) return { kind: 'other' };
    const statements =
      ts.isSourceFile(scope) || ts.isBlock(scope) || ts.isModuleBlock(scope)
        ? scope.statements
        : ts.isCaseClause(scope) || ts.isDefaultClause(scope)
          ? scope.statements
          : null;
    if (!statements) return null;
    for (const statement of statements) {
      if (ts.isImportDeclaration(statement)) {
        const clause = statement.importClause;
        const fromTable =
          ts.isStringLiteral(statement.moduleSpecifier) &&
          statement.moduleSpecifier.text === CHART_TYPE;
        const names: string[] = [];
        if (clause?.name) names.push(clause.name.text);
        const bindings = clause?.namedBindings;
        if (bindings && ts.isNamedImports(bindings))
          for (const element of bindings.elements) names.push(element.name.text);
        if (bindings && ts.isNamespaceImport(bindings)) names.push(bindings.name.text);
        if (names.includes(name)) return fromTable ? { kind: 'table' } : { kind: 'other' };
      }
      if (ts.isVariableStatement(statement)) {
        const isConst = (statement.declarationList.flags & ts.NodeFlags.Const) !== 0;
        for (const d of statement.declarationList.declarations) {
          const bound = boundNames(d.name).find((b) => b.name === name);
          if (!bound) continue;
          return isConst && d.initializer
            ? { kind: 'const', initializer: d.initializer, path: bound.path }
            : { kind: 'other' };
        }
      }
      if (ts.isFunctionDeclaration(statement) && statement.name?.text === name)
        return { kind: 'function', node: statement };
      if (
        (ts.isClassDeclaration(statement) || ts.isEnumDeclaration(statement)) &&
        statement.name?.text === name
      )
        return { kind: 'other' };
    }
    return null;
  };

  /** The binding a name read at `at` resolves to, the innermost scope first. */
  const resolve = (name: string, at: ts.Node): Binding => {
    for (let scope: ts.Node | undefined = at.parent; scope; scope = scope.parent) {
      const found = declaredIn(scope, name);
      if (found) return found;
    }
    return { kind: 'other' };
  };

  /**
   * A size from the table: a role read off an import (`SVG_TEXT.tick`, `SVG_TEXT[role]`), a choice
   * between such sizes (`compact ? SVG_TEXT.tick : SVG_TEXT.label`), or a const holding one, found
   * in the scope the name is read in (one step: a const of a const is no longer the table's).
   * Arithmetic, a call, a literal or a parameter is a size of its own.
   */
  const fromTable = (expression: ts.Expression, depth = 0): boolean => {
    if (ts.isParenthesizedExpression(expression)) return fromTable(expression.expression, depth);
    if (ts.isConditionalExpression(expression))
      return fromTable(expression.whenTrue, depth) && fromTable(expression.whenFalse, depth);
    let root: ts.Expression = expression;
    while (ts.isPropertyAccessExpression(root) || ts.isElementAccessExpression(root))
      root = root.expression;
    if (!ts.isIdentifier(root)) return false;
    const binding = resolve(root.text, root);
    if (binding.kind === 'table') return true;
    if (binding.kind !== 'const' || root !== expression || depth > 0) return false;
    if (binding.path.length === 0) return fromTable(binding.initializer, depth + 1);
    // `const { tick } = SVG_TEXT`: a role read off the table by its pattern
    let init: ts.Expression = binding.initializer;
    while (ts.isPropertyAccessExpression(init) || ts.isElementAccessExpression(init))
      init = init.expression;
    return ts.isIdentifier(init) && resolve(init.text, init).kind === 'table';
  };

  /** A size written as a string from the table: `String(SVG_TEXT.tick)` or `${SVG_TEXT.tick}`. */
  const stringFromTable = (expression: ts.Expression): boolean => {
    if (
      ts.isCallExpression(expression) &&
      ts.isIdentifier(expression.expression) &&
      expression.expression.text === 'String' &&
      expression.arguments.length === 1
    )
      return fromTable(expression.arguments[0]);
    if (ts.isTemplateExpression(expression))
      return (
        expression.head.text === '' &&
        expression.templateSpans.length === 1 &&
        expression.templateSpans[0].literal.text === '' &&
        fromTable(expression.templateSpans[0].expression)
      );
    return fromTable(expression);
  };

  const out: Offence[] = [];
  const at = (node: ts.Node) =>
    source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
  const offence = (node: ts.Node, what = node.getText(source)) =>
    out.push({ line: at(node), what: what.replace(/\s+/g, ' ').slice(0, 60) });
  const judge = (node: ts.Node, value: ts.Expression | undefined, what?: string) => {
    if (value === undefined || !fromTable(value)) offence(node, what);
  };

  /** A property key's name: plain, quoted, or computed from a literal; null when it has none. */
  const keyName = (name: ts.PropertyName): string | null => {
    if (ts.isIdentifier(name) || ts.isStringLiteral(name) || ts.isNumericLiteral(name))
      return name.text;
    if (ts.isComputedPropertyName(name) && ts.isStringLiteralLike(name.expression))
      return name.expression.text;
    return null;
  };

  /** A `const` object literal a name stands for, in its scope; null when it is none. */
  const constObject = (name: ts.Identifier): ts.ObjectLiteralExpression | null => {
    const binding = resolve(name.text, name);
    return binding.kind === 'const' &&
      binding.path.length === 0 &&
      ts.isObjectLiteralExpression(binding.initializer)
      ? binding.initializer
      : null;
  };

  /**
   * The size keys of a style object on SVG text. When the object is not written at the element
   * (`style={bad}`, `...bad`) `site` is where it is read, the line an offence is told at, and its
   * label is said after the key (`fontSize: 10 (style={bad})`).
   */
  const judgeStyle = (
    object: ts.ObjectLiteralExpression,
    site?: { node: ts.Node; label: string }
  ) => {
    const report = (property: ts.Node) =>
      offence(
        site?.node ?? property,
        site ? `${property.getText(source)} (${site.label})` : property.getText(source)
      );
    for (const property of object.properties) {
      if (ts.isSpreadAssignment(property)) {
        const spread = property.expression;
        const inner = ts.isIdentifier(spread) ? constObject(spread) : null;
        if (inner)
          judgeStyle(inner, {
            node: site?.node ?? property,
            label: `...${spread.getText(source)}`,
          });
        else report(property);
        continue;
      }
      if (ts.isShorthandPropertyAssignment(property)) {
        if (SIZE_KEYS.has(property.name.text) && !fromTable(property.name)) report(property);
        continue;
      }
      if (!ts.isPropertyAssignment(property)) continue;
      const key = keyName(property.name);
      if (key === null || key === 'font') report(property);
      else if (SIZE_KEYS.has(key) && !fromTable(property.initializer)) report(property);
    }
  };

  /** The function a name calls, in its scope: a function declaration or a `const` one. */
  const functionOf = (name: ts.Identifier): ts.FunctionLikeDeclaration | null => {
    const binding = resolve(name.text, name);
    if (binding.kind === 'function') return binding.node;
    if (
      binding.kind === 'const' &&
      binding.path.length === 0 &&
      (ts.isArrowFunction(binding.initializer) || ts.isFunctionExpression(binding.initializer))
    )
      return binding.initializer;
    return null;
  };

  /** What a function returns: its concise body, or each `return` of its own (not of one inside). */
  const returned = (fn: ts.FunctionLikeDeclaration): ts.Expression[] => {
    if (!fn.body) return [];
    if (!ts.isBlock(fn.body)) return [fn.body];
    const out: ts.Expression[] = [];
    const walk = (node: ts.Node) => {
      if (ts.isReturnStatement(node) && node.expression) out.push(node.expression);
      if (!ts.isFunctionLike(node)) ts.forEachChild(node, walk);
    };
    ts.forEachChild(fn.body, walk);
    return out;
  };

  /**
   * Reads the object a JSX spread gives SVG text as its attributes (`site` the spread): an object
   * written there or in a `const`, either branch of a choice, what a function of the file returns.
   * Its size keys are judged as an attribute would be, its `style` as a style; event handlers and
   * positions pass. False where it cannot find the object: the spread is then the offence.
   */
  const readSpread = (
    expression: ts.Expression,
    site: ts.JsxSpreadAttribute,
    seen = new Set<ts.Node>()
  ): boolean => {
    const label = site.getText(source);
    if (ts.isParenthesizedExpression(expression))
      return readSpread(expression.expression, site, seen);
    if (ts.isConditionalExpression(expression))
      return (
        readSpread(expression.whenTrue, site, seen) && readSpread(expression.whenFalse, site, seen)
      );
    if (ts.isIdentifier(expression)) {
      const object = constObject(expression);
      return object !== null && readSpread(object, site, seen);
    }
    if (ts.isCallExpression(expression) && ts.isIdentifier(expression.expression)) {
      const fn = functionOf(expression.expression);
      if (!fn || seen.has(fn)) return false;
      seen.add(fn);
      const results = returned(fn);
      return results.length > 0 && results.every((r) => readSpread(r, site, seen));
    }
    if (!ts.isObjectLiteralExpression(expression)) return false;
    for (const property of expression.properties) {
      if (ts.isSpreadAssignment(property)) {
        if (!readSpread(property.expression, site, seen)) return false;
        continue;
      }
      const name = ts.isShorthandPropertyAssignment(property)
        ? property.name.text
        : ts.isPropertyAssignment(property)
          ? keyName(property.name)
          : 'a method';
      if (name === null) return false;
      const value = ts.isShorthandPropertyAssignment(property)
        ? property.name
        : ts.isPropertyAssignment(property)
          ? property.initializer
          : null;
      if (name === 'font' || (SIZE_KEYS.has(name) && value && !fromTable(value)))
        offence(site, `${property.getText(source)} (${label})`);
      if (name === 'style' && value) {
        const style = ts.isObjectLiteralExpression(value)
          ? value
          : ts.isIdentifier(value)
            ? constObject(value)
            : null;
        if (!style) return false;
        judgeStyle(style, { node: site, label });
      }
    }
    return true;
  };

  /** Whether a JSX element holds SVG text among its children, at any depth. */
  const holdsSvgText = (element: ts.JsxElement): boolean => {
    let found = false;
    const look = (node: ts.Node) => {
      if (found) return;
      if (
        (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) &&
        SVG_WORD_TAGS.has(node.tagName.getText(source))
      )
        found = true;
      else ts.forEachChild(node, look);
    };
    element.children.forEach(look);
    return found;
  };

  const visit = (node: ts.Node) => {
    if (ts.isJsxAttribute(node)) {
      const name = node.name.getText(source);
      const tag = node.parent.parent.tagName.getText(source);
      const init = node.initializer;
      const value = init && ts.isJsxExpression(init) ? init.expression : init;
      if (SIZE_KEYS.has(name)) judge(node, value);
      else if (name === 'font') offence(node);
      else if (name === 'style' && SVG_TEXT_TAGS.has(tag)) {
        if (value && ts.isObjectLiteralExpression(value)) judgeStyle(value);
        else if (value && ts.isIdentifier(value) && constObject(value))
          judgeStyle(constObject(value)!, { node, label: `style={${value.text}}` });
        else offence(node);
      }
    }
    if (ts.isJsxSpreadAttribute(node)) {
      const element = node.parent.parent;
      const tag = element.tagName.getText(source);
      const spreadsOnWords =
        SVG_WORD_TAGS.has(tag) ||
        (SVG_TEXT_TAGS.has(tag) && ts.isJsxOpeningElement(element) && holdsSvgText(element.parent));
      if (spreadsOnWords && !readSpread(node.expression, node)) offence(node);
    }
    if (
      ts.isCallExpression(node) &&
      ts.isPropertyAccessExpression(node.expression) &&
      node.expression.name.text === 'setAttribute' &&
      node.arguments.length === 2 &&
      ts.isStringLiteralLike(node.arguments[0]) &&
      ['font-size', 'font'].includes(node.arguments[0].text) &&
      (node.arguments[0].text === 'font' || !stringFromTable(node.arguments[1]))
    )
      offence(node);
    if (
      (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) &&
      MARKUP_SIZE.test(node.text)
    ) {
      const m = /\bfont(?:-size)?\s*=\s*["'{\\][^"'}\s>]*["'}]?/.exec(node.text)!;
      offence(node, m[0]);
    }
    if (ts.isTemplateExpression(node)) {
      const text = [node.head.text, ...node.templateSpans.map((s) => s.literal.text)].join('${}');
      const m = /\bfont(?:-size)?\s*=\s*["'{\\][^"'}\s>]*["'}]?/.exec(text);
      if (m) offence(node, m[0]);
    }
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

  it('reads a spread it can find: handlers and positions pass, a size inside one does not', () => {
    const code = [
      "import { SVG_TEXT } from '@/lib/chart-type';",
      'function handlers(pin: { hover(): void }) {',
      '  return { onPointerMove: () => pin.hover(), onClick() {} };',
      '}',
      'const sized = () => ({ fontSize: 10 });',
      'export function A(across: boolean, pin: { hover(): void }) {',
      '  return (<svg {...handlers(pin)}>',
      "    <text {...(across ? { x: 1, textAnchor: 'middle' } : { transform: 'rotate(-90)' })}>a</text>",
      '    <text {...sized()}>b</text>',
      '    <text {...{ style: { fontSize: SVG_TEXT.tick } }}>c</text>',
      '  </svg>);',
      '}',
    ].join('\n');
    expect(offSvgSizes('a.tsx', code)).toEqual([{ line: 9, what: 'fontSize: 10 ({...sized()})' }]);
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

  it('scans the page source, .tsx and .ts', () => {
    expect(files.filter((f) => f.endsWith('.tsx')).length).toBeGreaterThan(250);
    expect(files.filter((f) => f.endsWith('.ts')).length).toBeGreaterThan(100);
  });

  it('finds no SVG fontSize that does not come from lib/chart-type.ts', () => {
    expect(unlisted(offences, { ...ALLOWLIST, ...UNREAD_SPREADS })).toEqual([]);
  });

  it('keeps the allowlists reasoned, current and their length pinned', () => {
    // one site keeps a size of its own; a new one is a ruling, not a line added here
    expect(Object.keys(ALLOWLIST)).toHaveLength(1);
    // two spreads read by hand; a third is read by the gate or ruled
    expect(Object.keys(UNREAD_SPREADS)).toHaveLength(2);
    for (const [key, { reason }] of Object.entries({ ...ALLOWLIST, ...UNREAD_SPREADS })) {
      expect(reason.trim().length, key).toBeGreaterThan(10);
      expect(
        offences.map((o) => o.key),
        key
      ).toContain(key);
    }
  });
});
