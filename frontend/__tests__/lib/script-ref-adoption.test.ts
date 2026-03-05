import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const TARGET_FILES = [
  'app/script/[codeHash]/client-page.tsx',
  'app/cell/[outpoint]/client-page.tsx',
  'app/scripts/page.tsx',
  'app/scripts/[name]/client-page.tsx',
  'app/tx/[hash]/client-page.tsx',
  'components/ui/script-view.tsx',
];

function readTarget(path: string): string {
  return readFileSync(join(process.cwd(), path), 'utf8');
}

describe('script-ref adoption', () => {
  it('keeps key script pages wired to shared script-ref helpers', () => {
    for (const file of TARGET_FILES) {
      const content = readTarget(file);
      expect(content).toMatch(
        /getScriptRefBadgeLabel|getScriptRefQueryHashType|normalizeScriptRefHashType/
      );
    }
  });

  it('avoids direct raw hashType rendering in JSX', () => {
    for (const file of TARGET_FILES) {
      const content = readTarget(file);
      expect(content).not.toMatch(/>\s*\{\s*script\.hashType\s*\}\s*</);
    }
  });
});
