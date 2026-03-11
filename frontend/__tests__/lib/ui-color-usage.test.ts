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

// Files that have been intentionally migrated to the new palette tokens
// (text-text-dim replaces text-text-muted as the canonical dim text color)
const migratedFiles = new Set([
  'components/ui/terminal-panel.tsx',
  'components/ui/stat-block.tsx',
  'components/search-bar.tsx',
  'components/command-palette.tsx',
]);

function findUnexpectedDimUsages(root: string): string[] {
  const files = walkFiles(root);
  const offenders: string[] = [];

  for (const file of files) {
    const relative = path.relative(process.cwd(), file);
    if (migratedFiles.has(relative)) continue;
    const content = fs.readFileSync(file, 'utf8');
    if (content.includes('text-text-dim')) {
      offenders.push(relative);
    }
  }

  return offenders.sort();
}

describe('ui color usage guard', () => {
  it('does not use text-text-dim in non-migrated app and components views', () => {
    const frontendRoot = process.cwd();
    const appOffenders = findUnexpectedDimUsages(path.join(frontendRoot, 'app'));
    const componentOffenders = findUnexpectedDimUsages(path.join(frontendRoot, 'components'));
    const offenders = [...appOffenders, ...componentOffenders];

    expect(offenders).toEqual([]);
  });
});
