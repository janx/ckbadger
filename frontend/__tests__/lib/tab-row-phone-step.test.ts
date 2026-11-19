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

  // Lanes C2: as a row moved onto the library's Tabs (components/ui/tabs.tsx, which sets its own
  // phone step by measurement) its file left this list, on purpose: the asset feed's row (lane
  // c2-assets), the scripts list's roles and the transactions list's views (lane c2-lists), the
  // block page's and the address page's rows (lane c2-chain). Five rows to none: no page draws a
  // section-size tab row by hand any more, and a row that comes back fails here.
  it('finds no hand-built tab row: every one is the library’s Tabs', () => {
    expect([...found.keys()].sort()).toEqual([]);
  });

  // Lane cd-fixes-controls (review-cd-tests NOTE-5): the two tests that stood here, a returning
  // row's 13 px label and 11 px count below md, iterated `found`, which is empty, so they could not
  // fail (and a returning row fails the test above). In their place the reader's own test: a
  // fixture source whose hand-built tab rows it must find, and whose other classes it must pass.
  it('finds a hand-built section-size tab in a fixture source, and passes what is not one', () => {
    const fixture = [
      "const TAB = 'text-section border-b-2 pb-2';",
      'export const A = ({ on }: { on: boolean }) => (',
      '  <button className={`text-section max-md:text-body border-b-2 ${on ? "a" : "b"}`}>a</button>',
      ');',
      "const HEADING = 'text-section pb-2';",
      "const RULE = 'text-body border-b-2';",
      "const LARGER = 'text-section-lg border-b-2';",
    ].join('\n');
    expect(sectionTabs(fixture)).toEqual([
      "'text-section border-b-2 pb-2'",
      '`text-section max-md:text-body border-b-2 ${on ? "a" : "b"}`',
    ]);
  });
});
