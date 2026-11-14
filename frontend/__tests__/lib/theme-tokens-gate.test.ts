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

/**
 * A hex colour: `#` and 3, 4, 6 or 8 hex digits (`#fff`, `#ffff`, `#2edba3`, `#2edba3cc`), where no
 * word character, `&` (an entity) or `/` (a URL's fragment) comes before the `#`, and no word
 * character, `-` or a digit group (`,009`, `.5`) after the digits. A run of digits alone (`#123`,
 * `#1234`) is an issue or a block number as often as a colour, so it counts only where nothing
 * but a colour stands: the whole of a string, or after `:` `(` `,` or a CSS length or border
 * style (`1px solid #000`).
 */
const HEX =
  /(?<![\w&/#])#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{4}|[0-9a-fA-F]{3})(?![\w-]|[,.]\d)/g;
const COLOUR_PLACE = /(?:[(,:]|\b(?:px|em|rem|solid|dashed|dotted|double|inset))\s*$/;

/** The hex colours in one line of code. */
function hexColours(line: string): string[] {
  return [...line.matchAll(HEX)]
    .filter((m) => {
      if (/[a-fA-F]/.test(m[0])) return true;
      const before = line.slice(0, m.index);
      const after = line.slice(m.index + m[0].length);
      const wholeString = /['"`]$/.test(before) && /^['"`]/.test(after);
      return wholeString || COLOUR_PLACE.test(before);
    })
    .map((m) => m[0]);
}

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
    expect(stripped.split('\n').flatMap(hexColours)).toEqual(['#2edba3']);
  });

  it('flags a hex colour of 3, 4, 6 or 8 digits, in a string, an attribute or a CSS value', () => {
    const lines = [
      "const a = { color: '#fff', background: '#FFF' };",
      '<rect fill="#0008" stroke="#abcd" />',
      "const c = '0 0 2px #2edba3, inset 0 0 0 1px #2EDBA3';",
      "const d = 'linear-gradient(#10131cff, #10131c80)';",
      "const e = { color: '#000', border: '1px solid #111', fill: 'rgb(0 0 0)' };",
      'const f = `color-mix(in srgb, #123 40%, transparent)`;',
    ];
    expect(lines.map(hexColours)).toEqual([
      ['#fff', '#FFF'],
      ['#0008', '#abcd'],
      ['#2edba3', '#2EDBA3'],
      ['#10131cff', '#10131c80'],
      ['#000', '#111'],
      ['#123'],
    ]);
  });

  it('passes what only looks like one: an anchor, an issue, a block number, an id, a fragment', () => {
    const lines = [
      '<a href="#top">top</a> <a href="#cell-0">cell 0</a>',
      "const issue = 'fixed in #123, see #4567 for the reason';",
      "const block = 'block #21,009,134 and #210,000 and #1.5';",
      "const id = document.querySelector('#seal-12'); const u = '/tx/0xabc#cafe';",
      "const entity = '&#123;'; const runs = '#12345 #1234567 #abcdefg';",
    ];
    expect(lines.map(hexColours)).toEqual([[], [], [], [], []]);
  });

  // The review's item F: the widening to 3, 4 and 8 digits (a34f664c) made the gate laxer for six:
  // main's matcher flagged '#' and six hex digits anywhere, and these passed after it. For six
  // digits the gate is at least as strict as main's; the context rule is for 3, 4 and 8 alone and
  // still catches those lengths in these positions.
  it('flags every place main flagged six digits, and the 3, 4 and 8 digit twins', () => {
    const six = [
      "const a = '0 1px 0 #000000';",
      "const b = 'inset 0 0 0 1px #111111';",
      "const c = 'drop-shadow(0 0 2px #000000)';",
      "const d = '#202020 1px solid';",
      'const e = `${a} #123456`;',
      '<div className="shadow-[0_0_4px_#2edba3]" />',
    ];
    expect(six.map(hexColours)).toEqual([
      ['#000000'],
      ['#111111'],
      ['#000000'],
      ['#202020'],
      ['#123456'],
      ['#2edba3'],
    ]);
    const twins = [
      "const a = '0 1px 0 #000';",
      "const b = 'inset 0 0 0 1px #1111';",
      "const c = 'drop-shadow(0 0 2px #00000080)';",
      "const d = '#202 1px solid';",
      'const e = `${a} #fff`;',
      '<div className="shadow-[0_0_4px_#2ed] ring-[#000]" />',
    ];
    expect(twins.map(hexColours)).toEqual([
      ['#000'],
      ['#1111'],
      ['#00000080'],
      ['#202'],
      ['#fff'],
      ['#2ed', '#000'],
    ]);
  });

  it('flags in the source every six-digit hex main’s matcher flags', () => {
    const MAIN_HEX = /#[0-9a-fA-F]{6}\b/g;
    const missed: string[] = [];
    for (const file of sourceFiles()) {
      const path = relative(ROOT, file);
      if (path === PALETTE_FILE) continue;
      stripComments(readFileSync(file, 'utf8'))
        .split('\n')
        .forEach((line, i) => {
          const found = hexColours(line);
          for (const m of line.matchAll(MAIN_HEX))
            if (!found.includes(m[0])) missed.push(`${path}:${i + 1} ${m[0]}`);
        });
    }
    expect(missed).toEqual([]);
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
        for (const hex of hexColours(line)) offences.push(`${path}:${i + 1} ${hex}`);
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
