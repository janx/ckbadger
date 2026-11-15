import ts from 'typescript';

/**
 * The AST reading behind the unit gate (A4 round 2): a date, a time, a shortened id or a count is
 * one unit wherever it is written as text. What the source shows exactly: a call to the formatter
 * that writes one, or a field that holds a date, inside an expression the JSX renders as text, with
 * nothing on the way that renders it as a unit.
 */
export const DATE_FORMATTERS = new Set([
  'formatUtc8Date',
  'formatUtc8DateTime',
  'formatUtc8Time',
  'formatUnixSecondsUtc8',
]);
export const ID_SHORTENERS = new Set([
  'shortenHash',
  'shortenId',
  'shortenAddress',
  'shortenOutpoint',
]);
export const COUNT_FORMATTERS = new Set(['formatCount']);
/** Fields the API and the pages fill with a `YYYY-MM-DD` day or a date (the ones that can be named). */
export const DATE_FIELDS = new Set([
  'date',
  'day',
  'weekStart',
  'activationDate',
  'completeThrough',
  'from',
  'through',
]);
/** Elements that render what they hold as a unit, or text that never wraps (SVG). */
const UNIT_ELEMENTS = new Set([
  'NoBreak',
  'ShortId',
  'time',
  'Utc8Date',
  'Utc8DateTime',
  'Utc8Age',
  'svg',
  'text',
  'tspan',
  'title',
]);
/** Calls that render a string's units (components/ui/units.tsx). */
const UNIT_RENDERERS = new Set(['unitText', 'unitNode']);
/**
 * A class that keeps its text on one line, in a className's source text (quotes, template or cn()
 * call around it): whitespace-nowrap, or Tailwind's truncate, which is white-space: nowrap with
 * overflow hidden and an ellipsis (A4 round 4). A breakpoint's (sm:truncate) is not one line at
 * every width.
 */
const NOWRAP = /(^|[\s'"`])(?:whitespace-nowrap|truncate)([\s'"`]|$)/;

function calleeName(call: ts.CallExpression): string {
  const e = call.expression;
  if (ts.isIdentifier(e)) return e.text;
  if (ts.isPropertyAccessExpression(e)) return e.name.text;
  return '';
}

function classText(el: ts.JsxOpeningLikeElement): string {
  const attr = el.attributes.properties.find(
    (p): p is ts.JsxAttribute => ts.isJsxAttribute(p) && p.name.getText() === 'className'
  );
  return attr?.initializer?.getText() ?? '';
}

/**
 * The JSX child expression that renders `node` as text: the nearest JsxExpression on the way up
 * whose parent is an element or a fragment (not an attribute), reached without passing through a
 * JSX element (whose own children are read on their own). Null when the value is not rendered as a
 * JSX child here (an attribute, a variable, a function's return).
 */
function renderingChild(node: ts.Node): ts.JsxExpression | null {
  for (let n: ts.Node = node.parent; n; n = n.parent) {
    if (ts.isJsxExpression(n))
      return ts.isJsxElement(n.parent) || ts.isJsxFragment(n.parent) ? n : null;
    if (ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n) || ts.isJsxFragment(n)) return null;
    if (ts.isFunctionLike(n)) return null;
  }
  return null;
}

/** Whether a unit renderer call wraps `node` on its way up to `child`. */
function renderedAsUnits(node: ts.Node, child: ts.JsxExpression): boolean {
  for (let n: ts.Node = node.parent; n && n !== child; n = n.parent)
    if (ts.isCallExpression(n) && UNIT_RENDERERS.has(calleeName(n))) return true;
  return false;
}

/** Whether an element around the child renders it as a unit (or as SVG text). */
function insideUnit(child: ts.JsxExpression): boolean {
  for (let n: ts.Node | undefined = child.parent; n; n = n.parent) {
    if (ts.isJsxElement(n)) {
      const el = n.openingElement;
      if (UNIT_ELEMENTS.has(el.tagName.getText())) return true;
      if (NOWRAP.test(classText(el))) return true;
    }
  }
  return false;
}

export interface UnitFinding {
  line: number;
  kind: 'date' | 'id' | 'count' | 'field' | 'decoration';
  text: string;
}

/**
 * Whether `node` stands in text position on its way up to `top`: the value itself, a template
 * literal's span, a concatenation, a ternary's branch — never a call's argument or a property's
 * object, whose result need not carry it.
 */
function inTextPosition(node: ts.Node, top: ts.Node): boolean {
  for (let n: ts.Node = node; n !== top; n = n.parent) {
    const p = n.parent;
    if (p === top) return true;
    const passes =
      ts.isTemplateSpan(p) ||
      ts.isTemplateExpression(p) ||
      ts.isParenthesizedExpression(p) ||
      (ts.isBinaryExpression(p) && p.operatorToken.kind === ts.SyntaxKind.PlusToken) ||
      (ts.isConditionalExpression(p) && n !== p.condition);
    if (!passes) return false;
  }
  return true;
}

/**
 * A value named `date` used as a value (not a property's name in `x.date`, not a declaration's or
 * a parameter's name, not an attribute's or an object key's).
 */
function isDateValue(n: ts.Node): n is ts.Identifier {
  if (!ts.isIdentifier(n) || n.text !== 'date') return false;
  const p = n.parent;
  if (ts.isPropertyAccessExpression(p) && p.name === n) return false;
  if ((ts.isVariableDeclaration(p) || ts.isParameter(p) || ts.isBindingElement(p)) && p.name === n)
    return false;
  if (ts.isPropertyAssignment(p) && p.name === n) return false;
  if (ts.isJsxAttribute(p)) return false;
  return true;
}

/** The units an expression writes, read by the same rules (its formatter calls, date fields). */
function unitsIn(expr: ts.Node): UnitFinding['kind'] | null {
  let kind: UnitFinding['kind'] | null = null;
  const visit = (n: ts.Node) => {
    if (kind) return;
    // JSX held in a name is read where it is written: its own children are checked there
    if (ts.isJsxElement(n) || ts.isJsxSelfClosingElement(n) || ts.isJsxFragment(n)) return;
    if (ts.isCallExpression(n)) {
      const name = calleeName(n);
      if (DATE_FORMATTERS.has(name)) kind = 'date';
      else if (ID_SHORTENERS.has(name)) kind = 'id';
      else if (COUNT_FORMATTERS.has(name)) kind = 'count';
    }
    if (ts.isPropertyAccessExpression(n) && DATE_FIELDS.has(n.name.text) && inTextPosition(n, expr))
      kind = 'field';
    if (isDateValue(n) && inTextPosition(n, expr)) kind = 'date';
    ts.forEachChild(n, visit);
  };
  visit(expr);
  return kind;
}

/**
 * The value a name stands for in this file, where the source says it exactly: the nearest
 * enclosing `const name = …` (a block's, a function's or the file's), and when that is a call to a
 * function the file declares by name, that function's body. Null for a parameter, an import or
 * anything declared elsewhere.
 */
function resolveConst(name: string, from: ts.Node): ts.Node | null {
  for (let n: ts.Node | undefined = from.parent; n; n = n.parent) {
    if (!(ts.isBlock(n) || ts.isSourceFile(n))) continue;
    for (const statement of n.statements) {
      if (ts.isFunctionDeclaration(statement) && statement.name?.text === name)
        return statement.body ?? null;
      if (!ts.isVariableStatement(statement)) continue;
      if (!(statement.declarationList.flags & ts.NodeFlags.Const)) continue;
      for (const d of statement.declarationList.declarations)
        if (ts.isIdentifier(d.name) && d.name.text === name && d.initializer) {
          const init = d.initializer;
          if (ts.isArrowFunction(init) || ts.isFunctionExpression(init)) return init.body;
          if (ts.isCallExpression(init) && ts.isIdentifier(init.expression))
            return resolveConst(init.expression.text, n.statements[0] ?? n) ?? init;
          return init;
        }
    }
  }
  return null;
}

const DECORATION = /(^|[\s'"`:])(underline|line-through|overline)([\s'"`]|$)/;

export function unprotectedUnits(file: ts.SourceFile): UnitFinding[] {
  const found: UnitFinding[] = [];
  const push = (node: ts.Node, kind: UnitFinding['kind']) =>
    found.push({
      line: file.getLineAndCharacterOfPosition(node.getStart()).line + 1,
      kind,
      text: node.getText().replace(/\s+/g, ' ').slice(0, 90),
    });
  const report = (node: ts.Node, kind: UnitFinding['kind']) => {
    const child = renderingChild(node);
    if (!child || renderedAsUnits(node, child) || insideUnit(child)) return;
    push(node, kind);
  };
  const visit = (node: ts.Node) => {
    if (ts.isCallExpression(node)) {
      const name = calleeName(node);
      if (DATE_FORMATTERS.has(name)) report(node, 'date');
      else if (ID_SHORTENERS.has(name)) report(node, 'id');
      else if (COUNT_FORMATTERS.has(name)) report(node, 'count');
    }
    // a date field rendered as it is: `{peak.date}` (not `{x.date.slice(..)}`'s inner access)
    if (
      ts.isPropertyAccessExpression(node) &&
      DATE_FIELDS.has(node.name.text) &&
      ts.isJsxExpression(node.parent) &&
      (ts.isJsxElement(node.parent.parent) || ts.isJsxFragment(node.parent.parent))
    )
      report(node, 'field');
    // a value named `date` written into the text: `{date}`, `${date}` (A4 round 3)
    if (isDateValue(node)) {
      const child = renderingChild(node);
      if (child?.expression && inTextPosition(node, child.expression)) report(node, 'date');
    }
    // a name the file declares, rendered as it is: `{title}` where `const title = …` writes a
    // unit, directly or through the file's own function it calls (A4 round 3)
    if (
      ts.isJsxExpression(node) &&
      node.expression &&
      ts.isIdentifier(node.expression) &&
      (ts.isJsxElement(node.parent) || ts.isJsxFragment(node.parent)) &&
      !insideUnit(node)
    ) {
      const value = resolveConst(node.expression.text, node);
      const kind = value ? unitsIn(value) : null;
      if (kind) push(node, kind);
    }
    // a unit renderer's child under a decoration it cannot take: the decoration is on a farther
    // element, not on its parent (an inline block takes only its parent's, by inherit) (A4 round 3)
    if (ts.isCallExpression(node) && UNIT_RENDERERS.has(calleeName(node))) {
      const child = renderingChild(node);
      if (child && ts.isJsxElement(child.parent)) {
        const parentDecorated = DECORATION.test(classText(child.parent.openingElement));
        let farther = false;
        for (let n: ts.Node | undefined = child.parent.parent; n; n = n.parent)
          if (ts.isJsxElement(n) && DECORATION.test(classText(n.openingElement))) farther = true;
        if (farther && !parentDecorated) push(node, 'decoration');
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return found;
}
