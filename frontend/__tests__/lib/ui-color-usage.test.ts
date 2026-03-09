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

function findSlate600Usages(root: string): string[] {
  const files = walkFiles(root);
  const offenders: string[] = [];

  for (const file of files) {
    const content = fs.readFileSync(file, 'utf8');
    if (content.includes('text-text-dim')) {
      offenders.push(path.relative(process.cwd(), file));
    }
  }

  return offenders.sort();
}

describe('ui color usage guard', () => {
  it('does not use text-text-dim in app and components views', () => {
    const frontendRoot = process.cwd();
    const appOffenders = findSlate600Usages(path.join(frontendRoot, 'app'));
    const componentOffenders = findSlate600Usages(path.join(frontendRoot, 'components'));
    const offenders = [...appOffenders, ...componentOffenders];

    expect(offenders).toEqual([]);
  });
});
