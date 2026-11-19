import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * PA-17 (UI review 2026-11-14): a row of tabs at the section size (17 px, the underlined
 * `border-b-2` tab) takes one type step down below md — the label 13 px (`max-md:text-body`), its
 * count 11 px (`max-md:text-meta`) — on every page alike, so the address page's three tabs hold
 * one row at 360 and no other tab row stays a step larger. A new tab row at the section size must
 * take the same step; the controls lane (C1) replaces these rows with one component.
 */
const ROOT = join(__dirname, '..', '..');
const SCAN_DIRS = ['app', 'components'];

function sourceFiles(): string[] {
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (name.endsWith('.tsx')) files.push(path);
    }
  };
  for (const dir of SCAN_DIRS) walk(join(ROOT, dir));
  return files;
}

/** Every one-line string or template literal in the source (a class list is one). */
const LITERAL = /'[^'\n]*'|`[^`\n]*`|"[^"\n]*"/g;

/** The class strings of section-size tabs: 17 px text with the 2 px underline of a tab. */
function sectionTabs(code: string): string[] {
  return (code.match(LITERAL) ?? []).filter(
    (s) => /(^|\s|['"`])text-section(\s|['"`])/.test(s) && /(^|\s)border-b-2(\s|['"`])/.test(s)
  );
}

describe('section-size tab rows take one type step down below md (PA-17)', () => {
  const found = new Map<string, string[]>();
  for (const file of sourceFiles()) {
    const tabs = sectionTabs(readFileSync(file, 'utf8'));
    if (tabs.length) found.set(relative(ROOT, file), tabs);
  }

  // Lanes C2: as a row moves onto the library's Tabs (components/ui/tabs.tsx, which sets its own
  // phone step by measurement) its file leaves this list, on purpose: the block page's row did.
  it('finds the hand-built tab rows of the site', () => {
    expect([...found.keys()].sort()).toEqual([
      'app/address/[addr]/client-page.tsx',
      'app/scripts/page.tsx',
      'app/transactions/page.tsx',
      'components/asset/asset-feed.tsx',
    ]);
  });

  it('sets every section-size tab 13 px below md', () => {
    const offences = [...found].flatMap(([file, tabs]) =>
      tabs.filter((t) => !/(^|\s)max-md:text-body(\s|['"`])/.test(t)).map((t) => `${file}: ${t}`)
    );
    expect(offences).toEqual([]);
  });

  it('sets every count in those rows 11 px below md, and no count stays 12 px', () => {
    const offences = [...found.keys()].flatMap((file) => {
      const code = readFileSync(join(ROOT, file), 'utf8');
      // A tab's count: the dim, normal-weight figure beside the label.
      return (code.match(LITERAL) ?? [])
        .filter((s) => /text-text-dim/.test(s) && /text-small/.test(s) && /font-normal/.test(s))
        .filter((s) => !/(^|\s)max-md:text-meta(\s|['"`])/.test(s))
        .map((s) => `${file}: ${s}`);
    });
    expect(offences).toEqual([]);
  });
});
