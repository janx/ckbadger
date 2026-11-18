import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Gate (readability review 2026-11-18, lane A): a metric's figure is one unit. On a `.cell`
 * identity at 1280 "CKB held in its cell" broke into "240" / ".00000000 CKB": the metric gave its
 * amount `ExactCkb wrap` (the renderer is `CkbAmount` since lane B2), which lets a line break
 * between an amount's integer and its decimals.
 * A metric (an object literal with a `label` and a `value`, the shape of `AssetMetric`) never
 * gives its value a wrapping amount; its row holds the figure whole (`AssetMetrics`).
 */
function parse(code: string): ts.SourceFile {
  return ts.createSourceFile('t.tsx', code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
}

/** Whether a JSX element is `CkbAmount` with `wrap` set (bare, or to anything but `false`). */
function wrappingAmount(node: ts.Node): boolean {
  const el = ts.isJsxElement(node)
    ? node.openingElement
    : ts.isJsxSelfClosingElement(node)
      ? node
      : null;
  if (!el || el.tagName.getText() !== 'CkbAmount') return false;
  return el.attributes.properties.some(
    (p) =>
      ts.isJsxAttribute(p) &&
      p.name.getText() === 'wrap' &&
      p.initializer?.getText().replace(/\s/g, '') !== '{false}'
  );
}

/** Each metric literal whose value holds a wrapping amount, as `line: label`. */
export function wrappingMetrics(file: ts.SourceFile): string[] {
  const found: string[] = [];
  const visit = (node: ts.Node) => {
    if (ts.isObjectLiteralExpression(node)) {
      const prop = (name: string) =>
        node.properties.find(
          (p): p is ts.PropertyAssignment => ts.isPropertyAssignment(p) && p.name.getText() === name
        );
      const label = prop('label');
      const value = prop('value');
      if (label && value) {
        let wraps = false;
        const scan = (n: ts.Node) => {
          if (wrappingAmount(n)) wraps = true;
          if (!wraps) ts.forEachChild(n, scan);
        };
        scan(value.initializer);
        if (wraps)
          found.push(
            `${file.getLineAndCharacterOfPosition(value.getStart()).line + 1}: ${label.initializer.getText()}`
          );
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return found;
}

describe('the metric amount gate reads the AST exactly (self-tests)', () => {
  const scan = (code: string) => wrappingMetrics(parse(code));
  it('finds a metric whose value is an amount that may wrap, however the value is chosen', () => {
    expect(
      scan(`const m = { label: 'CKB held', value: <CkbAmount form="column" shannons={h} wrap /> };`)
    ).toEqual(["1: 'CKB held'"]);
    expect(
      scan(
        `const m = { label: 'held', value: h ? <CkbAmount form="column" shannons={h} wrap={true} /> : 'none' };`
      )
    ).toEqual(["1: 'held'"]);
  });
  it('passes an amount held whole, a non-metric object and an amount in the note', () => {
    expect(
      scan(`const m = { label: 'held', value: <CkbAmount form="column" shannons={h} /> };`)
    ).toEqual([]);
    expect(
      scan(
        `const m = { label: 'held', value: <CkbAmount form="column" shannons={h} wrap={false} /> };`
      )
    ).toEqual([]);
    expect(
      scan(
        `const c = { key: 'held', render: () => <CkbAmount form="column" shannons={h} wrap /> };`
      )
    ).toEqual([]);
    expect(
      scan(
        `const m = { label: 'held', value: '1', note: <CkbAmount form="column" shannons={h} wrap /> };`
      )
    ).toEqual([]);
  });
});

const ROOT = join(__dirname, '..', '..');
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx$/.test(name) ? [path] : [];
  });
}

describe('every metric holds its amount whole (readability 2026-11-18)', () => {
  it('gives no metric value an amount that breaks between its integer and its decimals', () => {
    const found: string[] = [];
    for (const dir of ['app', 'components'])
      for (const path of sources(join(ROOT, dir))) {
        const rel = relative(ROOT, path);
        found.push(...wrappingMetrics(parse(readFileSync(path, 'utf8'))).map((f) => `${rel}:${f}`));
      }
    expect(found).toEqual([]);
  });
});
