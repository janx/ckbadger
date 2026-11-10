import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';
import { MATERIAL_KEYS, PALETTE_KEYS } from '@/lib/palette';

/**
 * Gate for the theme (the light theme, 2026-11-10): page code writes no colour as a hex literal.
 * Every colour is a theme reference — a Tailwind class, a `color()` / `alpha()` / `material()`
 * from lib/palette.ts, or a `var(--c-…)` string — so both themes draw it; a canvas reads the
 * palette through `useThemePalette()`. lib/palette.ts is the one place hex lives. Comments are
 * not code: a hex named in words stays.
 */

const ROOT = join(__dirname, '..', '..');
const SCAN_DIRS = ['app', 'components', 'hooks', 'lib', 'src'];
const PALETTE_FILE = 'lib/palette.ts';
/** Sites the gate tolerates, as `path` → one-line reason. Empty: the palette holds every hex. */
const ALLOWLIST: Record<string, string> = {};

function sourceFiles(): string[] {
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.(tsx|ts|css)$/.test(name) && !name.endsWith('.d.ts')) files.push(path);
    }
  };
  for (const dir of SCAN_DIRS) walk(join(ROOT, dir));
  return files;
}

/** The code of a source: block comments and line comments removed. */
export function stripComments(code: string): string {
  return code.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:'"`\\])\/\/.*$/gm, '$1');
}

const HEX = /#[0-9a-fA-F]{6}\b/g;
const THEME_VAR = /var\(--(c|rgb|m)-([a-z0-9-]+)\)/g;

describe('the hex gate finds what it must, and only that', () => {
  it('strips comments but keeps strings and URLs', () => {
    const code = [
      "const url = 'https://example.org/x'; // the row-rule ink #1a1f30",
      '/* the quiet slate #4b5266 */ const a = 1;',
      "const b = '#2edba3';",
    ].join('\n');
    const stripped = stripComments(code);
    expect(stripped).toContain("'https://example.org/x'");
    expect(stripped).not.toContain('#1a1f30');
    expect(stripped).not.toContain('#4b5266');
    expect(stripped.match(HEX)).toEqual(['#2edba3']);
  });
});

describe('every colour is a theme reference (the light theme)', () => {
  const files = sourceFiles();
  const offences: string[] = [];
  const unknownVars: string[] = [];
  const known = new Set<string>([
    ...PALETTE_KEYS.map((k) => `c-${k}`),
    ...PALETTE_KEYS.map((k) => `rgb-${k}`),
    ...MATERIAL_KEYS.map((k) => `m-${k}`),
  ]);
  for (const file of files) {
    const path = relative(ROOT, file);
    const code = stripComments(readFileSync(file, 'utf8'));
    if (path !== PALETTE_FILE)
      code.split('\n').forEach((line, i) => {
        for (const m of line.matchAll(HEX)) offences.push(`${path}:${i + 1} ${m[0]}`);
      });
    for (const m of code.matchAll(THEME_VAR))
      if (!known.has(`${m[1]}-${m[2]}`)) unknownVars.push(`${path} ${m[0]}`);
  }

  it('scans the page source and the stylesheet', () => {
    expect(files.length).toBeGreaterThan(300);
    expect(files.some((f) => f.endsWith('globals.css'))).toBe(true);
  });

  it('writes no hex colour outside lib/palette.ts', () => {
    expect(offences.filter((o) => !(o.split(':')[0] in ALLOWLIST))).toEqual([]);
  });

  it('references only colours and materials the palette declares', () => {
    expect(unknownVars).toEqual([]);
  });

  it('keeps the allowlist reasoned and current', () => {
    for (const [site, reason] of Object.entries(ALLOWLIST)) {
      expect(reason.trim().length, site).toBeGreaterThan(10);
      expect(
        offences.map((o) => o.split(':')[0]),
        site
      ).toContain(site);
    }
  });
});
