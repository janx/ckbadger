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
/** The class in a className's source text (quotes, template or cn() call around it). */
const NOWRAP = /(^|[\s'"`])whitespace-nowrap([\s'"`]|$)/;

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
  kind: 'date' | 'id' | 'count' | 'field';
  text: string;
}

export function unprotectedUnits(file: ts.SourceFile): UnitFinding[] {
  const found: UnitFinding[] = [];
  const report = (node: ts.Node, kind: UnitFinding['kind']) => {
    const child = renderingChild(node);
    if (!child || renderedAsUnits(node, child) || insideUnit(child)) return;
    found.push({
      line: file.getLineAndCharacterOfPosition(node.getStart()).line + 1,
      kind,
      text: node.getText().replace(/\s+/g, ' ').slice(0, 90),
    });
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
    ts.forEachChild(node, visit);
  };
  visit(file);
  return found;
}
