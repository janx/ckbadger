import { describe, expect, it } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';

function walkFiles(dir: string): string[] {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files: string[] = [];

  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...walkFiles(fullPath));
      continue;
    }

    if (entry.isFile() && /\.(ts|tsx|js|jsx)$/.test(entry.name)) {
      files.push(fullPath);
    }
  }

  return files;
}

// Deprecated tokens that should no longer appear in source files.
// These have been replaced by the new palette tokens:
//   text-text-muted    -> text-text-dim
//   text-text-secondary -> text-text
//   text-text-primary   -> text-text-bright
const DEPRECATED_TOKENS = ['text-text-muted', 'text-text-secondary', 'text-text-primary'];

function findDeprecatedTokenUsages(root: string): string[] {
  const files = walkFiles(root);
  const offenders: string[] = [];

  for (const file of files) {
    const relative = path.relative(process.cwd(), file);
    const content = fs.readFileSync(file, 'utf8');
    for (const token of DEPRECATED_TOKENS) {
      if (content.includes(token)) {
        offenders.push(`${relative} (${token})`);
      }
    }
  }

  return offenders.sort();
}

describe('ui color usage guard', () => {
  it('does not use deprecated color tokens in app and components views', () => {
    const frontendRoot = process.cwd();
    const appOffenders = findDeprecatedTokenUsages(path.join(frontendRoot, 'app'));
    const componentOffenders = findDeprecatedTokenUsages(path.join(frontendRoot, 'components'));
    const offenders = [...appOffenders, ...componentOffenders];

    expect(offenders).toEqual([]);
  });
});
