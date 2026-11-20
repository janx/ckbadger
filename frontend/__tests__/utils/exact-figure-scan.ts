import ts from 'typescript';
import { unwrap } from '@/__tests__/utils/source-scan';

/**
 * The reader of the exact-figure gate (`__tests__/lib/exact-figure-gate.test.ts`; lane exactness,
 * part A, A8): every call in a source that rounds a number or reads a decimal into a float, where
 * its result may be written. Kinds:
 *
 * - `toFixed`, `toPrecision`: any call (they make a string; a coordinate in a path is one too);
 * - `round`: `Math.round`, `Math.floor`, `Math.ceil`, `Math.trunc` whose result reaches text;
 * - `fraction-digits`: `toLocaleString(…)` or `new Intl.NumberFormat(…)` given a fraction or a
 *   significant-digit option (`minimumFractionDigits`, `maximumFractionDigits`,
 *   `minimumSignificantDigits`, `maximumSignificantDigits`) in an object literal;
 * - `locale-rounds` (lane exactness, part A2, the test review's NOTE-2): `toLocaleString(…)` or
 *   `new Intl.NumberFormat(…)` with no such option that rounds all the same: given `style:
 *   'percent'` (whole percents) or `notation: 'compact'`, or a `toLocaleString` of a quotient
 *   (`(a / b).toLocaleString()`: a float, rounded at three places);
 * - `number-to-text`: `Number(…)`, `parseFloat(…)`, `Number.parseFloat(…)` or a unary `+` whose
 *   result reaches text.
 *
 * A value reaches text where it is (through parentheses, type assertions, arithmetic, `?:`, `??`,
 * `||` and `&&`) a template literal's expression, a JSX child, a JSX attribute a reader is told
 * (`title`, `aria-label`, `aria-valuetext`, `alt`, `placeholder`; any other attribute is geometry or
 * a component's prop), the argument of `String(…)`, the operand of `+` beside a string literal or a
 * template, or the object of `.toString()`, `.toLocaleString()`, `.toFixed()` or `.toPrecision()`. A
 * name bound to such a value (`const x = Math.round(…)`, through the same chain) reaches text where
 * one of its reads does. What it cannot see is said by the gate.
 */
export type ExactKind =
  | 'toFixed'
  | 'toPrecision'
  | 'round'
  | 'fraction-digits'
  | 'locale-rounds'
  | 'number-to-text';

export interface ExactSite {
  kind: ExactKind;
  line: number;
  text: string;
}

const ROUNDERS = new Set(['round', 'floor', 'ceil', 'trunc']);
const DIGIT_OPTIONS = new Set([
  'minimumFractionDigits',
  'maximumFractionDigits',
  'minimumSignificantDigits',
  'maximumSignificantDigits',
]);
const TEXT_METHODS = new Set(['toString', 'toLocaleString', 'toFixed', 'toPrecision']);
/** The JSX attributes whose value a reader is told (a title, an accessible name or value). */
const TEXT_ATTRIBUTES = new Set(['title', 'aria-label', 'aria-valuetext', 'alt', 'placeholder']);
/** Locale options that round whatever they are given: whole percents, a compact form. */
const ROUNDING_OPTIONS: readonly [string, string][] = [
  ['style', 'percent'],
  ['notation', 'compact'],
];

/** Whether an options argument names a digit option. */
const hasDigitOption = (args: readonly ts.Expression[]) =>
  args.some(
    (a) =>
      ts.isObjectLiteralExpression(a) &&
      a.properties.some(
        (pr) => pr.name !== undefined && ts.isIdentifier(pr.name) && DIGIT_OPTIONS.has(pr.name.text)
      )
  );
/** Whether an options argument names a style that rounds (`style: 'percent'`, a compact form). */
const hasRoundingOption = (args: readonly ts.Expression[]) =>
  args.some(
    (a) =>
      ts.isObjectLiteralExpression(a) &&
      a.properties.some(
        (pr) =>
          ts.isPropertyAssignment(pr) &&
          ts.isIdentifier(pr.name) &&
          ts.isStringLiteralLike(pr.initializer) &&
          ROUNDING_OPTIONS.some(
            ([name, value]) =>
              (pr.name as ts.Identifier).text === name &&
              (pr.initializer as ts.StringLiteralLike).text === value
          )
      )
  );
/** Whether an expression is, or holds in its arithmetic, a quotient: a float, whatever its operands. */
const hasQuotient = (e: ts.Expression): boolean => {
  const u = unwrap(e);
  if (ts.isBinaryExpression(u))
    return (
      u.operatorToken.kind === ts.SyntaxKind.SlashToken ||
      hasQuotient(u.left) ||
      hasQuotient(u.right)
    );
  if (ts.isConditionalExpression(u)) return hasQuotient(u.whenTrue) || hasQuotient(u.whenFalse);
  if (ts.isPrefixUnaryExpression(u)) return hasQuotient(u.operand);
  return false;
};

export function exactSites(code: string, kind: ts.ScriptKind = ts.ScriptKind.TSX): ExactSite[] {
  const source = ts.createSourceFile('exact.tsx', code, ts.ScriptTarget.Latest, true, kind);
  const lineOf = (n: ts.Node) => source.getLineAndCharacterOfPosition(n.getStart(source)).line + 1;
  const text = (n: ts.Node) => n.getText(source).replace(/\s+/g, ' ');

  const isMathRound = (n: ts.Node): n is ts.CallExpression =>
    ts.isCallExpression(n) &&
    ts.isPropertyAccessExpression(n.expression) &&
    ts.isIdentifier(n.expression.expression) &&
    n.expression.expression.text === 'Math' &&
    ROUNDERS.has(n.expression.name.text);
  const isNumberRead = (n: ts.Node): n is ts.Expression =>
    (ts.isCallExpression(n) &&
      ((ts.isIdentifier(n.expression) &&
        (n.expression.text === 'Number' || n.expression.text === 'parseFloat')) ||
        // Number.parseFloat(…)
        (ts.isPropertyAccessExpression(n.expression) &&
          ts.isIdentifier(n.expression.expression) &&
          n.expression.expression.text === 'Number' &&
          n.expression.name.text === 'parseFloat'))) ||
    // a unary + reads its operand into a float (`+s`); a numeric literal is none
    (ts.isPrefixUnaryExpression(n) &&
      n.operator === ts.SyntaxKind.PlusToken &&
      !ts.isNumericLiteral(n.operand));

  /** The expression `n` stands in, up the value chain: what its value becomes part of. */
  const carrier = (n: ts.Node): ts.Node => {
    let x: ts.Node = n;
    for (;;) {
      const p = x.parent;
      if (!p) return x;
      if (
        ts.isParenthesizedExpression(p) ||
        ts.isAsExpression(p) ||
        ts.isSatisfiesExpression(p) ||
        ts.isNonNullExpression(p) ||
        ts.isTypeAssertionExpression(p) ||
        ts.isPrefixUnaryExpression(p)
      ) {
        x = p;
        continue;
      }
      if (ts.isConditionalExpression(p) && p.condition !== x) {
        x = p;
        continue;
      }
      if (ts.isBinaryExpression(p)) {
        const op = p.operatorToken.kind;
        if (
          op === ts.SyntaxKind.PlusToken ||
          op === ts.SyntaxKind.MinusToken ||
          op === ts.SyntaxKind.AsteriskToken ||
          op === ts.SyntaxKind.SlashToken ||
          op === ts.SyntaxKind.PercentToken ||
          op === ts.SyntaxKind.QuestionQuestionToken ||
          op === ts.SyntaxKind.BarBarToken ||
          op === ts.SyntaxKind.AmpersandAmpersandToken
        ) {
          x = p;
          continue;
        }
      }
      return x;
    }
  };
  const isStringy = (e: ts.Expression) => {
    const u = unwrap(e);
    return ts.isStringLiteralLike(u) || ts.isTemplateExpression(u);
  };
  /** Whether the value of `n` is written: the place its carrier stands in is text. */
  const writes = (n: ts.Node): boolean => {
    // a `+` beside a string anywhere up the chain makes it text
    for (let x: ts.Node = n; x !== carrier(n) && x.parent; x = x.parent) {
      const p = x.parent;
      if (
        ts.isBinaryExpression(p) &&
        p.operatorToken.kind === ts.SyntaxKind.PlusToken &&
        (isStringy(p.left) || isStringy(p.right))
      )
        return true;
    }
    const c = carrier(n);
    const p = c.parent;
    if (!p) return false;
    if (ts.isTemplateSpan(p) && p.expression === c) return true;
    if (ts.isJsxExpression(p) && p.expression === c)
      return (
        ts.isJsxElement(p.parent) ||
        ts.isJsxFragment(p.parent) ||
        (ts.isJsxAttribute(p.parent) &&
          ts.isIdentifier(p.parent.name) &&
          TEXT_ATTRIBUTES.has(p.parent.name.text))
      );
    if (
      ts.isCallExpression(p) &&
      ts.isIdentifier(p.expression) &&
      p.expression.text === 'String' &&
      p.arguments[0] === c
    )
      return true;
    if (
      ts.isPropertyAccessExpression(p) &&
      p.expression === c &&
      TEXT_METHODS.has(p.name.text) &&
      ts.isCallExpression(p.parent)
    )
      return true;
    return false;
  };

  // ---- names bound to a rounded value, to a fixed point ----
  const bound = new Map<string, ts.Node>(); // name -> the call it holds
  const origin = (n: ts.Node): ts.Node | null => {
    if (isMathRound(n) || isNumberRead(n)) return n;
    if (ts.isIdentifier(n) && bound.has(n.text)) return bound.get(n.text)!;
    return null;
  };
  let before = -1;
  while (bound.size !== before) {
    before = bound.size;
    const bind = (n: ts.Node): void => {
      const o = origin(n);
      if (o) {
        const c = carrier(n);
        const p = c.parent;
        if (p && ts.isVariableDeclaration(p) && p.initializer === c && ts.isIdentifier(p.name))
          if (!bound.has(p.name.text)) bound.set(p.name.text, o);
      }
      ts.forEachChild(n, bind);
    };
    bind(source);
  }

  const found: ExactSite[] = [];
  const seen = new Set<string>();
  const add = (kind: ExactKind, at: ts.Node) => {
    const key = `${kind}:${at.pos}`;
    if (seen.has(key)) return;
    seen.add(key);
    found.push({ kind, line: lineOf(at), text: text(at) });
  };
  const visit = (n: ts.Node): void => {
    if (ts.isCallExpression(n) && ts.isPropertyAccessExpression(n.expression)) {
      const name = n.expression.name.text;
      if (name === 'toFixed' || name === 'toPrecision') add(name, n);
      if (name === 'toLocaleString') {
        if (hasDigitOption(n.arguments)) add('fraction-digits', n);
        else if (hasRoundingOption(n.arguments) || hasQuotient(n.expression.expression))
          add('locale-rounds', n);
      }
    }
    if (
      ts.isNewExpression(n) &&
      ts.isPropertyAccessExpression(n.expression) &&
      ts.isIdentifier(n.expression.expression) &&
      n.expression.expression.text === 'Intl' &&
      n.expression.name.text === 'NumberFormat'
    ) {
      if (hasDigitOption(n.arguments ?? [])) add('fraction-digits', n);
      else if (hasRoundingOption(n.arguments ?? [])) add('locale-rounds', n);
    }
    // a rounded value, or a name holding one, that is written
    const o = origin(n);
    if (o && (ts.isIdentifier(n) ? !isDeclarationName(n) : true) && writes(n))
      add(isMathRound(o) ? 'round' : 'number-to-text', o);
    ts.forEachChild(n, visit);
  };
  visit(source);
  return found.sort((a, b) => a.line - b.line || a.kind.localeCompare(b.kind));
}

/** Whether an identifier is the name a declaration binds, not a read of it. */
function isDeclarationName(n: ts.Identifier): boolean {
  const p = n.parent;
  return (
    (ts.isVariableDeclaration(p) && p.name === n) ||
    (ts.isParameter(p) && p.name === n) ||
    (ts.isBindingElement(p) && p.name === n) ||
    (ts.isPropertyAssignment(p) && p.name === n) ||
    (ts.isFunctionDeclaration(p) && p.name === n) ||
    (ts.isPropertyAccessExpression(p) && p.name === n)
  );
}
