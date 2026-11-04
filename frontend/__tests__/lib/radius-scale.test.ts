import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * Gate for the three corner radii (visual finish 2026-11-04, ruling V10): page code rounds a
 * corner only with `rounded-sm` (2 px — controls, chips, small tags), `rounded` (4 px — the
 * default), `rounded-lg` (8 px — glass frames) or `rounded-full` (dots and pills), in every
 * direction (`rounded-t`, `rounded-l-sm`, …). Tailwind's other steps (`rounded-md`, `rounded-xl`,
 * `rounded-2xl`, `rounded-3xl`) and arbitrary radii (`rounded-[3px]`) are off the system. The CSS
 * radii of `.well` (3 px) and the header search field's `--well-radius` (6 px) are board-approved
 * and live in `globals.css`, which this gate does not read.
 */

/** Off-system sites the gate tolerates, as `path:line` → one-line reason. Empty by ruling V10. */
const ALLOWLIST: Record<string, string> = {};

const SCAN_DIRS = ['app', 'components'];
/** An off-system radius class in any variant and any direction (`sm:rounded-tl-xl`). */
const OFF_SYSTEM =
  /(?<![\w-])(?:[\w-]+:)*rounded(?:-(?:[trblse]|tl|tr|bl|br|ss|se|es|ee))?-(?:md|xl|2xl|3xl|\[[^\]\s]*\])(?![\w-])/g;

/** Every off-system radius class in a source text (comments included: keep them in words). */
function offSystemRadii(code: string): { line: number; what: string }[] {
  const out: { line: number; what: string }[] = [];
  code.split('\n').forEach((text, i) => {
    for (const m of text.matchAll(OFF_SYSTEM)) out.push({ line: i + 1, what: m[0] });
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

describe('the radius gate finds what it must, and only that', () => {
  it('flags the off-system steps and arbitrary radii in any variant and direction', () => {
    const code = [
      '<div className="rounded-md border rounded-xl sm:rounded-2xl md:hover:rounded-3xl">',
      "const t = 'rounded-[3px] rounded-l-[4px] rounded-tl-md rounded-e-xl rounded-[0.4rem]';",
    ].join('\n');
    expect(offSystemRadii(code)).toEqual([
      { line: 1, what: 'rounded-md' },
      { line: 1, what: 'rounded-xl' },
      { line: 1, what: 'sm:rounded-2xl' },
      { line: 1, what: 'md:hover:rounded-3xl' },
      { line: 2, what: 'rounded-[3px]' },
      { line: 2, what: 'rounded-l-[4px]' },
      { line: 2, what: 'rounded-tl-md' },
      { line: 2, what: 'rounded-e-xl' },
      { line: 2, what: 'rounded-[0.4rem]' },
    ]);
  });

  it('passes the four steps in every direction and look-alike words', () => {
    const code = [
      '<i className="rounded rounded-sm rounded-lg rounded-full rounded-none sm:rounded-lg">',
      "const t = 'rounded-t rounded-l-sm rounded-br-lg rounded-r-full max-w-2xl shadow-xl';",
      '// the corners are rounded-medium in prose, and text-xl-wide is not a radius',
    ].join('\n');
    expect(offSystemRadii(code)).toEqual([]);
  });
});

describe('three corner radii (V10)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const offences = files.flatMap((file) => {
    const path = relative(root, file);
    return offSystemRadii(readFileSync(file, 'utf8')).map((o) => `${path}:${o.line} ${o.what}`);
  });
  const key = (offence: string) => offence.split(' ')[0];

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(300);
  });

  it('rounds no corner off the four steps (rounded-sm, rounded, rounded-lg, rounded-full)', () => {
    expect(offences.filter((o) => !(key(o) in ALLOWLIST))).toEqual([]);
  });

  it('keeps the allowlist reasoned and current', () => {
    for (const [site, reason] of Object.entries(ALLOWLIST)) {
      expect(reason.trim().length, site).toBeGreaterThan(10);
      expect(offences.map(key), site).toContain(site);
    }
  });
});
