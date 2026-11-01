import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

/**
 * Site-wide guard for the touch rule (plan R6, `docs/RESPONSIVE_LAYOUT.md`, Touch): anything a
 * hover reveals is also reachable by a tap, so no user-facing text asks for a hover alone, and the
 * words "tap or hover" are written once, in the hint module, so every hint reads the same.
 */
describe('hover hints always say "tap or hover", from one place', () => {
  const root = join(__dirname, '..', '..');
  const HINT = 'components/ui/tap-or-hover-hint.tsx';
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.(tsx|ts)$/.test(name) && !name.endsWith('.d.ts')) files.push(path);
    }
  };
  for (const dir of ['app', 'components', 'hooks', 'lib']) walk(join(root, dir));

  /** Attributes whose values are never read by a person (classes, data hooks, geometry, links). */
  const MACHINE_ATTR =
    /^(className|key|id|href|src|role|type|rel|target|style|testId|data-[\w-]+|\w+ClassName|viewBox|d|fill|stroke|transform|points|textAnchor|fontSize|paintOrder)$/;

  /** Every piece of text in a file a reader could see: string and template literals, JSX text. */
  function texts(file: string): { text: string; line: number }[] {
    const code = readFileSync(file, 'utf8');
    const source = ts.createSourceFile(
      file,
      code,
      ts.ScriptTarget.Latest,
      true,
      file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
    );
    const out: { text: string; line: number }[] = [];
    const visit = (node: ts.Node) => {
      if (
        ts.isImportDeclaration(node) ||
        ts.isExportDeclaration(node) ||
        ts.isLiteralTypeNode(node)
      )
        return;
      if (ts.isJsxAttribute(node) && MACHINE_ATTR.test(node.name.getText(source))) return;
      if (
        ts.isStringLiteral(node) ||
        ts.isNoSubstitutionTemplateLiteral(node) ||
        ts.isTemplateHead(node) ||
        ts.isTemplateMiddle(node) ||
        ts.isTemplateTail(node) ||
        ts.isJsxText(node)
      ) {
        const line = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
        out.push({ text: node.text.replace(/\s+/g, ' '), line });
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
    return out;
  }

  const all = files.flatMap((file) =>
    texts(file).map((t) => ({ ...t, file: relative(root, file) }))
  );

  it('reads the source it guards', () => {
    expect(all.length).toBeGreaterThan(1000);
    expect(files.map((f) => relative(root, f))).toContain(HINT);
  });

  it('finds no text that asks for a hover without a tap', () => {
    const offenders = all
      .filter(({ file }) => file !== HINT)
      // the English word, not `hover:` classes, `group-hover`, `data-hover` or `hover-dot` ids
      .filter(({ text }) => /(?<![-\w])hover(?![-:\w])/i.test(text))
      .map(({ file, line, text }) => `${file}:${line} ${text.trim()}`);
    expect(offenders).toEqual([]);
  });

  it('writes the words "tap or hover" only in the hint module', () => {
    const elsewhere = all
      .filter(({ file, text }) => file !== HINT && /tap\s+or\s+hover/i.test(text))
      .map(({ file, line, text }) => `${file}:${line} ${text.trim()}`);
    expect(elsewhere).toEqual([]);
  });
});
