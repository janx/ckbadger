import { readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import ts from 'typescript';

/**
 * The readers the source gates share (the controls gate, `__tests__/lib/controls-gate.test.ts`, and
 * the link-look gate, `__tests__/lib/link-look-gate.test.ts`; moved here by lane c2-finish, item 5,
 * with no expectation changed): the directories they scan and the files in them, what a name of a
 * file is bound to, and an expression with its parentheses and type assertions taken off.
 */

/** The directories a source gate reads. */
export const SCAN_DIRS = ['app', 'components', 'hooks', 'lib'] as const;

/** Every TypeScript source under `root`'s scanned directories, declaration files aside. */
export function sourceFiles(root: string): string[] {
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.tsx?$/.test(name) && !name.endsWith('.d.ts')) files.push(path);
    }
  };
  for (const dir of SCAN_DIRS) walk(join(root, dir));
  return files;
}

/** What a name of the file is bound to: a value it may hold, a component, or a prop (its default). */
export type Binding =
  | { kind: 'value'; values: ts.Expression[] }
  | { kind: 'component' }
  | { kind: 'prop'; defaults: ts.Expression[] };

/** An expression with its parentheses, `as`, `satisfies`, `<T>` and `!` taken off. */
export const unwrap = (e: ts.Expression): ts.Expression => {
  let x = e;
  while (
    ts.isParenthesizedExpression(x) ||
    ts.isAsExpression(x) ||
    ts.isSatisfiesExpression(x) ||
    ts.isTypeAssertionExpression(x) ||
    ts.isNonNullExpression(x)
  )
    x = x.expression;
  return x;
};
