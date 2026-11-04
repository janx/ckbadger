import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * Gate for colour in the token system (visual finish 2026-11-04, ruling V11): page code writes no
 * colour as a bracket hex class (`text-[#a0a8be]`, `border-l-[#4b5266]`, `from-[#4a3a12]/50`,
 * `shadow-[0_0_6px_#2edba3]`, `[background:#10131c]`). A value equal to a theme colour is that
 * colour's Tailwind class (`text-text`, `border-base-border-subtle`); any other value is a named
 * constant in `lib/design-tokens.ts`, applied through `style` or an SVG attribute. Hex in SVG
 * attributes, inline styles and `lib/` data is outside this gate: it scans class strings.
 */

/** Sites the gate tolerates, as `path:line` → one-line reason. Empty by ruling V11. */
const ALLOWLIST: Record<string, string> = {};

const SCAN_DIRS = ['app', 'components'];
const HEX = '#[0-9a-fA-F]{3,8}';
/**
 * A utility whose arbitrary value carries a hex, in any variant and with any opacity modifier
 * (`hover:border-[#6a8850]`, `text-[#a0a8be]/70`), or an arbitrary property with a hex in it.
 */
const BRACKET_HEX = new RegExp(
  String.raw`(?<![\w-])(?:[\w-]+:)*(?:[a-z][\w-]*-\[[^\]\s]*${HEX}[^\]\s]*\]|\[[a-z-]+:[^\]\s]*${HEX}[^\]\s]*\])(?:\/[\w.]+)?`,
  'g'
);

/** Every bracket-hex class in a source text (comments included: name colours in words). */
function bracketHexes(code: string): { line: number; what: string }[] {
  const out: { line: number; what: string }[] = [];
  code.split('\n').forEach((text, i) => {
    for (const m of text.matchAll(BRACKET_HEX)) out.push({ line: i + 1, what: m[0] });
  });
  return out;
}

function sourceFiles(root: string): string[] {
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (/\.(tsx|ts)$/.test(name) && !name.endsWith('.d.ts')) files.push(path);
    }
  };
  for (const dir of SCAN_DIRS) walk(join(root, dir));
  return files;
}

describe('the hex gate finds what it must, and only that', () => {
  it('flags a bracket hex on any colour utility, in any variant and with any modifier', () => {
    const code = [
      '<p className="text-[#a0a8be] bg-[#10131c] border-[#1a1f30] hover:text-[#c0d8a0]">',
      "const t = 'fill-[#2edba3] stroke-[#fff] from-[#4a3a12]/50 via-[#0e3830] to-[#000]';",
      "const u = 'ring-[#2edba3] outline-[#a0a8be] decoration-[#68ccf0]/35 border-l-[#a0a8be]';",
      "const v = 'shadow-[0_0_6px_#2edba3] md:group-hover:bg-[#06090f] [background:#10131c]';",
    ].join('\n');
    expect(bracketHexes(code).map((o) => `${o.line} ${o.what}`)).toEqual([
      '1 text-[#a0a8be]',
      '1 bg-[#10131c]',
      '1 border-[#1a1f30]',
      '1 hover:text-[#c0d8a0]',
      '2 fill-[#2edba3]',
      '2 stroke-[#fff]',
      '2 from-[#4a3a12]/50',
      '2 via-[#0e3830]',
      '2 to-[#000]',
      '3 ring-[#2edba3]',
      '3 outline-[#a0a8be]',
      '3 decoration-[#68ccf0]/35',
      '3 border-l-[#a0a8be]',
      '4 shadow-[0_0_6px_#2edba3]',
      '4 md:group-hover:bg-[#06090f]',
      '4 [background:#10131c]',
    ]);
  });

  it('passes token classes, style values, SVG attributes and hex in prose', () => {
    const code = [
      '<p className="text-text border-base-border-subtle bg-jade/10 text-[color:var(--ink)]">',
      '<rect fill="#10131c" style={{ color: \'#4b5266\' }} />',
      '// the quiet slate #4b5266 sits between ghost and dim; see [#123] for the reason',
      "const w = 'w-[3px] shadow-[0_14px_32px_rgba(0,0,0,0.68)] [overflow-wrap:anywhere]';",
    ].join('\n');
    expect(bracketHexes(code)).toEqual([]);
  });
});

describe('no colour as a bracket hex (V11)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const offences = files.flatMap((file) => {
    const path = relative(root, file);
    return bracketHexes(readFileSync(file, 'utf8')).map((o) => `${path}:${o.line} ${o.what}`);
  });
  const key = (offence: string) => offence.split(' ')[0];

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(300);
  });

  it('writes every colour as a theme class or a named design-token constant', () => {
    expect(offences.filter((o) => !(key(o) in ALLOWLIST))).toEqual([]);
  });

  it('keeps the allowlist reasoned and current', () => {
    for (const [site, reason] of Object.entries(ALLOWLIST)) {
      expect(reason.trim().length, site).toBeGreaterThan(10);
      expect(offences.map(key), site).toContain(site);
    }
  });
});
