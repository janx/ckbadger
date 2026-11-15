import ts from 'typescript';

/**
 * The AST reading behind the nowrap gate (A4 Chrome sweep): what makes an element a unit a line
 * never breaks inside, and two such units set side by side with no way for the line to break
 * between them, or a unit around a phrase of words.
 */

/** Renderers that draw their value as one unit (NoBreak, or a renderer built on it). */
const UNIT_RENDERERS = new Set(['NoBreak', 'ShortId', 'OutpointIndex', 'Utc8Date', 'Utc8DateTime']);
/** Renderers that are one unit when shortened (a `size` prop), a scroller box when whole. */
const SHORTENED_RENDERERS = new Set(['Hash', 'HexCode', 'Address']);

const NOWRAP = /(^|\s)whitespace-nowrap(\s|$)/;
const FLEX_OR_GRID = /(^|\s)(inline-)?(flex|grid)(\s|$)/;

/** Every string literal that makes up a className attribute (a literal, or cn()/template parts). */
export function classStrings(attr: ts.JsxAttribute | undefined): string[] {
  if (!attr?.initializer) return [];
  const out: string[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) out.push(node.text);
    else if (ts.isTemplateExpression(node)) {
      out.push(node.head.text);
      for (const span of node.templateSpans) out.push(span.literal.text);
    }
    ts.forEachChild(node, visit);
  };
  visit(attr.initializer);
  return out;
}

function attrOf(el: ts.JsxOpeningLikeElement, name: string): ts.JsxAttribute | undefined {
  return el.attributes.properties.find(
    (p): p is ts.JsxAttribute => ts.isJsxAttribute(p) && p.name.getText() === name
  );
}

function opening(node: ts.Node): ts.JsxOpeningLikeElement | null {
  if (ts.isJsxElement(node)) return node.openingElement;
  if (ts.isJsxSelfClosingElement(node)) return node;
  return null;
}

/** Whether a JSX child is one unit: a unit renderer, a shortened id renderer, or a nowrap class. */
export function isUnit(node: ts.Node): boolean {
  const el = opening(node);
  if (!el) return false;
  const tag = el.tagName.getText();
  if (UNIT_RENDERERS.has(tag)) return true;
  if (SHORTENED_RENDERERS.has(tag)) return attrOf(el, 'size') !== undefined;
  return classStrings(attrOf(el, 'className')).some((s) => NOWRAP.test(s));
}

/** Whether an element lays its children out as flex or grid items (a literal class says so). */
function isFlexOrGrid(el: ts.JsxOpeningLikeElement): boolean {
  return classStrings(attrOf(el, 'className')).some((s) => FLEX_OR_GRID.test(s));
}

/** A class that sets a box of its own line (unprefixed: below every breakpoint). */
const OWN_LINE = /(^|\s)(block|flex|grid|table|list-item)(\s|$)/;

/** Whether a unit stands in its line's text (not a block, flex or grid box of its own). */
function isInline(node: ts.Node): boolean {
  const el = opening(node);
  return el !== null && !classStrings(attrOf(el, 'className')).some((s) => OWN_LINE.test(s));
}

/**
 * The element whose box lays out a node's children: the node itself, or for a fragment the
 * nearest enclosing element (a fragment draws no box; its children are its parent's).
 */
function layoutBox(node: ts.JsxElement | ts.JsxFragment): ts.JsxOpeningLikeElement | null {
  if (ts.isJsxElement(node)) return node.openingElement;
  for (let n: ts.Node = node.parent; n; n = n.parent) {
    if (ts.isJsxElement(n)) return n.openingElement;
    const passes =
      ts.isJsxFragment(n) ||
      ts.isJsxExpression(n) ||
      ts.isParenthesizedExpression(n) ||
      ts.isConditionalExpression(n) ||
      ts.isBinaryExpression(n);
    if (!passes) return null;
  }
  return null;
}

/** JSX text that renders as nothing (whitespace with a line break is dropped by JSX). */
function rendersNothing(child: ts.JsxChild): boolean {
  if (ts.isJsxText(child)) return /^\s*$/.test(child.text) && /\n/.test(child.text);
  if (ts.isJsxExpression(child)) return child.expression === undefined; // a {/* comment */}
  return false;
}

export interface Finding {
  line: number;
  text: string;
}

/**
 * (a) Two units side by side in an inline line with nothing rendered between them: the line has
 * no break opportunity between two unbreakable units. Skipped: children of a flex or grid
 * container (each is an item; items break between), and an out point's index after its hash (one
 * unit by design, `hash:0`).
 */
export function adjacentUnits(file: ts.SourceFile): Finding[] {
  const found: Finding[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node) || ts.isJsxFragment(node)) {
      const box = layoutBox(node);
      if (!(box && isFlexOrGrid(box))) {
        const kids = node.children.filter((c) => !rendersNothing(c));
        for (let i = 1; i < kids.length; i++) {
          const [a, b] = [kids[i - 1], kids[i]];
          if (!isUnit(a) || !isUnit(b) || !isInline(a) || !isInline(b)) continue;
          if (opening(b)?.tagName.getText() === 'OutpointIndex') continue;
          found.push({
            line: file.getLineAndCharacterOfPosition(b.getStart()).line + 1,
            text: `${opening(a)!.tagName.getText()} + ${opening(b)!.tagName.getText()}`,
          });
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return found;
}

/**
 * (b) A unit around a phrase: a nowrap element (NoBreak, or a literal whitespace-nowrap class)
 * whose children are only literal text of two or more words. A phrase may break between its
 * words; the unit kinds (an id, an out point's index, a date, a number with its unit, a label with
 * its id) each carry a value, so they are never literal text alone.
 */
export function phraseUnits(file: ts.SourceFile): Finding[] {
  const found: Finding[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isJsxElement(node)) {
      const el = node.openingElement;
      const nowrap =
        el.tagName.getText() === 'NoBreak' ||
        classStrings(attrOf(el, 'className')).some((s) => NOWRAP.test(s));
      const kids = node.children.filter((c) => !rendersNothing(c));
      if (nowrap && kids.length > 0 && kids.every(ts.isJsxText)) {
        const text = kids
          .map((k) => (k as ts.JsxText).text)
          .join('')
          .trim();
        const words = text.split(/\s+/).filter((w) => /[A-Za-z]{2,}/.test(w));
        if (words.length >= 2)
          found.push({
            line: file.getLineAndCharacterOfPosition(node.getStart()).line + 1,
            text,
          });
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return found;
}
