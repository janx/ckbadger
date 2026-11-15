import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import resolveConfig from 'tailwindcss/resolveConfig';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';
import tailwindConfig from '@/tailwind.config';

/**
 * Gate for square corners (UI review 2026-11-14, U-02, ruling R1: zero, as the boards — their CSS
 * rounds nothing but a dot).
 *
 * (a) The Tailwind theme's radius scale is 0 at every step but `full`, so `rounded`, `rounded-sm`,
 *     `rounded-lg`, … in any direction draw a square corner. Such a step is inert, and page code
 *     writes none (lane E3): no class list holds a `rounded*` class other than `rounded…-full`,
 *     bare, sized or directional, under any variant (`sm:rounded-t-md`, `hover:rounded`), and no
 *     radius by a custom property (`[--well-radius:6px]`). A class list is a string literal or a
 *     template's text whose every word is class-shaped (lowercase, digits, `-_:/.!@*`, bracketed
 *     arbitrary values), or any string given to a `className` attribute; a comment, JSX text, and
 *     a string with words no class has (`'cut, never rounded.'`, a `title`) are not class lists.
 * (b) `rounded-full` is for a mark only: a dot or a ring of at most 14 px, square, with no glyph.
 *     The class string that holds it (the quoted literal on its line) fixes that size itself: every
 *     height / width class in it is a small size (`h-0.5` … `h-3.5`, `w-*` the same, `size-*`, or
 *     `h-[Npx]` / `w-[Npx]` with N ≤ 14), its unprefixed height equals its unprefixed width (or one
 *     `size-*` sets both), and it carries no type-scale class (`text-meta` … `text-display`): a
 *     round `?` is a control, not a mark, and is square. Anything else needs an ALLOWLIST entry.
 * (c) No arbitrary radius (`rounded-[3px]`, `[border-radius:6px]`): it would step around the theme.
 * (d) No radius outside the theme: no `borderRadius` in an inline style, and `globals.css`
 *     declares none but 0 (the `.well` is square).
 *
 * SVG corners (`rx` on a frame, card, tile or bar) are pinned by each component's own test; the
 * seal stones (lib/seal) are drawn, not styled, and are not corners of the page.
 */

/** Sites the gate tolerates, as `path:line` → why the element is a round mark whose size is set elsewhere. */
const ALLOWLIST: Record<string, string> = {};

/** Where the page's classes are written: the directories tailwind.config.ts reads for content. */
const SCAN_DIRS = ['app', 'components'];

/** The Tailwind spacing steps up to 14 px (0.25 rem per unit at a 16 px root). */
const SMALL_STEPS: Record<string, number> = {
  '0.5': 2,
  '1': 4,
  '1.5': 6,
  '2': 8,
  '2.5': 10,
  '3': 12,
  '3.5': 14,
};
const MAX_MARK_PX = 14;

const DIRECTIONS = '(?:-(?:[trblse]|tl|tr|bl|br|ss|se|es|ee))?';
/** `rounded-full` in any variant and any direction (`md:rounded-t-full`). */
const ROUND_FULL = new RegExp(`(?<![\\w-])(?:[\\w-]+:)*rounded${DIRECTIONS}-full(?![\\w-])`, 'g');
/** An arbitrary radius in any variant and direction, or an arbitrary border-radius property. */
const ARBITRARY = new RegExp(
  `(?<![\\w-])(?:[\\w-]+:)*(?:rounded${DIRECTIONS}-\\[[^\\]\\s]*\\]|\\[border-radius:[^\\]\\s]*\\])`,
  'g'
);
const INLINE_RADIUS = /\bborderRadius\s*:/g;
/** An inert radius step in any variant and direction (`rounded`, `sm:rounded-t-md`); not `-full`, not arbitrary. */
const INERT_STEP = new RegExp(
  `(?<![\\w-])(?:[\\w-]+:)*!?rounded${DIRECTIONS}(?:-(?:none|sm|md|lg|xl|2xl|3xl))?(?![\\w-])`,
  'g'
);
/** A radius by a custom property, in any variant (`[--well-radius:6px]`). */
const RADIUS_PROPERTY = /(?<![\w-])(?:[\w-]+:)*\[--[\w-]*radius[\w-]*:[^\]\s]*\]/g;
/** A class-shaped word: lowercase, digits and `-_:/.!@*` around bracketed arbitrary values, never ending in `.` or `:`. */
const CLASS_WORD = /^(?:[a-z0-9_:/.!@*-]|\[[^\]\s]*\])*(?:[a-z0-9_!*-]|\[[^\]\s]*\])$/;
/** The type scale's classes (lib/type-scale.ts): a class string that sets one sets text. */
const TYPE_CLASS = /^(?:[\w-]+:)*text-(?:meta|small|body|section|hero|index|entity|display)$/;
const SIZE_CLASS = /^((?:[\w-]+:)*)(h|w|size)-(.+)$/;

/** A size class's value in px when it is a small size, else null (`full`, `5`, `[20px]`). */
function smallPx(value: string): number | null {
  if (value in SMALL_STEPS) return SMALL_STEPS[value];
  const px = /^\[(\d+(?:\.\d+)?)px\]$/.exec(value);
  return px && Number(px[1]) <= MAX_MARK_PX ? Number(px[1]) : null;
}

/** Why a class string holding `rounded-full` is not a small round mark, or null when it is one. */
function notAMark(classes: string): string | null {
  const list = classes.split(/\s+/).filter(Boolean);
  if (list.some((c) => TYPE_CLASS.test(c))) return 'it sets text: a control, not a mark';
  let h: number | null = null;
  let w: number | null = null;
  for (const c of list) {
    const m = SIZE_CLASS.exec(c);
    if (!m) continue;
    const [, variant, axis, value] = m;
    const px = smallPx(value);
    if (px === null) return `${c} is not a mark's size (at most ${MAX_MARK_PX} px)`;
    if (variant) continue;
    if (axis !== 'w') h = px;
    if (axis !== 'h') w = px;
  }
  if (h === null || w === null) return 'its size is not fixed in the same class string';
  if (h !== w) return `${h} × ${w} px is not square`;
  return null;
}

/** The quoted literal around column `at` of a line (to the line's end for a template that runs on). */
function literalAround(line: string, at: number): string {
  let open = -1;
  for (let i = at - 1; i >= 0; i--) {
    if (line[i] === "'" || line[i] === '"' || line[i] === '`') {
      open = i;
      break;
    }
  }
  if (open < 0) return line;
  const close = line.indexOf(line[open], at);
  return line.slice(open + 1, close < 0 ? undefined : close);
}

interface Offence {
  line: number;
  what: string;
}

/** Every corner a source text rounds off the rule (comments included: keep them in words). */
function offRule(code: string): Offence[] {
  const out: Offence[] = [];
  code.split('\n').forEach((text, i) => {
    for (const m of text.matchAll(ROUND_FULL)) {
      const why = notAMark(literalAround(text, m.index));
      if (why) out.push({ line: i + 1, what: `${m[0]}: ${why}` });
    }
    for (const m of text.matchAll(ARBITRARY))
      out.push({ line: i + 1, what: `${m[0]}: an arbitrary radius` });
    for (const m of text.matchAll(INLINE_RADIUS))
      out.push({ line: i + 1, what: `${m[0]} an inline radius` });
  });
  return out;
}

/**
 * Every radius a class list in a source text writes around the square theme (rule a): an inert
 * step or a radius custom property. Comments and JSX text are not read; a string literal or a
 * template's text is read when it is a class list.
 */
function inertRadii(path: string, code: string): Offence[] {
  const source = ts.createSourceFile(
    path,
    code,
    ts.ScriptTarget.Latest,
    true,
    path.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  );
  const out: Offence[] = [];
  const visit = (node: ts.Node) => {
    if (
      ts.isStringLiteral(node) ||
      ts.isNoSubstitutionTemplateLiteral(node) ||
      ts.isTemplateHead(node) ||
      ts.isTemplateMiddle(node) ||
      ts.isTemplateTail(node)
    ) {
      const attribute = ts.isJsxAttribute(node.parent) ? node.parent.name.getText(source) : null;
      const words = node.text.split(/\s+/).filter(Boolean);
      const classList =
        attribute === null ? words.every((w) => CLASS_WORD.test(w)) : /className$/i.test(attribute);
      if (classList) {
        const first = source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1;
        const found = [
          ...[...node.text.matchAll(INERT_STEP)].map((m) => ({ m, why: 'an inert step' })),
          ...[...node.text.matchAll(RADIUS_PROPERTY)].map((m) => ({ m, why: 'a radius property' })),
        ].sort((a, b) => a.m.index - b.m.index);
        for (const { m, why } of found) {
          const line = first + (node.text.slice(0, m.index).match(/\n/g)?.length ?? 0);
          out.push({ line, what: `${m[0]}: ${why}` });
        }
      }
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
  return out;
}

/** The radii a stylesheet declares other than 0 (comments stripped). */
function cssRadii(css: string): string[] {
  const code = css.replace(/\/\*[\s\S]*?\*\//g, '');
  return [...code.matchAll(/border(?:-[a-z]+)*-radius\s*:\s*([^;]+);/g)]
    .map((m) => m[1].trim())
    .filter((v) => !/^0(?:px)?$/.test(v));
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

describe('the corner gate finds what it must, and only that', () => {
  it('flags a round bar, chip, control or large disc, and arbitrary and inline radii', () => {
    const code = [
      '<div className="bg-base-bg mt-2 h-1.5 w-full overflow-hidden rounded-full">',
      "  'inline-flex h-5 w-5 items-center justify-center rounded-full border',",
      "const Q = 'ml-1 inline-flex h-3.5 w-3.5 rounded-full border font-mono text-meta';",
      "cn('inline-flex rounded-full border', size === 'sm' ? 'h-[18px]' : 'h-[22px]')",
      "'h-3 w-2 rounded-full' 'h-2 w-2 md:h-5 md:w-5 rounded-full' 'rounded-t-full h-2'",
      '<span className="h-[15px] w-[15px] rounded-full" />',
      "const t = 'rounded-[3px] sm:rounded-l-[4px] [border-radius:6px]';",
      'style={{ borderRadius: `${size * 0.14}px` }}',
    ].join('\n');
    expect(offRule(code)).toEqual([
      { line: 1, what: "rounded-full: w-full is not a mark's size (at most 14 px)" },
      { line: 2, what: "rounded-full: h-5 is not a mark's size (at most 14 px)" },
      { line: 3, what: 'rounded-full: it sets text: a control, not a mark' },
      { line: 4, what: 'rounded-full: its size is not fixed in the same class string' },
      { line: 5, what: 'rounded-full: 12 × 8 px is not square' },
      { line: 5, what: "rounded-full: md:h-5 is not a mark's size (at most 14 px)" },
      { line: 5, what: 'rounded-t-full: its size is not fixed in the same class string' },
      { line: 6, what: "rounded-full: h-[15px] is not a mark's size (at most 14 px)" },
      { line: 7, what: 'rounded-[3px]: an arbitrary radius' },
      { line: 7, what: 'sm:rounded-l-[4px]: an arbitrary radius' },
      { line: 7, what: '[border-radius:6px]: an arbitrary radius' },
      { line: 8, what: 'borderRadius: an inline radius' },
    ]);
  });

  it('passes small square marks, the inert steps and look-alike words', () => {
    const code = [
      "<span aria-hidden className={cn('h-1.5 w-1.5 rounded-full', STATE_DOT[state])} />",
      "const DOT_BOX = 'h-[7px] w-[7px] shrink-0 self-center rounded-full';",
      '<span className="border-text-dim h-[9px] w-[9px] rounded-full border-[1.2px]" />',
      "'flex h-3 w-3 shrink-0 items-center justify-center rounded-full border' 'size-3.5 rounded-full'",
      '<i className="rounded rounded-sm rounded-lg rounded-md rounded-none sm:rounded-t-lg">',
      "const t = 'max-w-2xl shadow-xl text-xl-wide [--well-radius:6px]';",
    ].join('\n');
    expect(offRule(code)).toEqual([]);
  });

  it('reads the radii a stylesheet declares, comments aside', () => {
    const css = [
      '/* border-radius: 3px in words */',
      '.well { border-radius: var(--well-radius, 3px); }',
      '.a { border-top-left-radius: 2px; } .b { border-radius: 0; } .c { border-radius: 0px; }',
    ].join('\n');
    expect(cssRadii(css)).toEqual(['var(--well-radius, 3px)', '2px']);
  });

  it('flags every inert step and radius property in a class list, in any variant and direction', () => {
    const code = [
      '<div className="rounded border px-2" />',
      "const BOX = 'flex rounded-lg sm:rounded-t-md';",
      "cn('hover:rounded', active && 'lg:rounded-l-none rounded-sm first:rounded-r')",
      'const t = `text-small rounded-tl-xl ${tone} md:!rounded`;',
      "const f = 'well h-9 pl-7 [--well-radius:6px] focus:shadow-[0_0_0_1px_rgb(var(--rgb-jade)/0.12)]';",
      '<span className="h-1.5 w-1.5 rounded-full">a sentence, rounded.</span>',
    ].join('\n');
    expect(inertRadii('fixture.tsx', code)).toEqual([
      { line: 1, what: 'rounded: an inert step' },
      { line: 2, what: 'rounded-lg: an inert step' },
      { line: 2, what: 'sm:rounded-t-md: an inert step' },
      { line: 3, what: 'hover:rounded: an inert step' },
      { line: 3, what: 'lg:rounded-l-none: an inert step' },
      { line: 3, what: 'rounded-sm: an inert step' },
      { line: 3, what: 'first:rounded-r: an inert step' },
      { line: 4, what: 'rounded-tl-xl: an inert step' },
      { line: 4, what: 'md:!rounded: an inert step' },
      { line: 5, what: '[--well-radius:6px]: a radius property' },
    ]);
  });

  it('leaves marks, look-alike words, comments, JSX text and strings that are not class lists', () => {
    const code = [
      "<span aria-hidden className={cn('h-1.5 w-1.5 rounded-full', STATE_DOT[state])} />",
      "const WORDS = 'surrounded grounded rounded-up';",
      '// the tall rounded stone; the `[--well-radius:6px]` the field once set, `rounded-lg`',
      '/* rounded up to a round number */',
      '<p>rounded half up</p>',
      '<abbr title="rounded half up">≈</abbr>',
      "const NOTE = 'Amounts are cut, never rounded.';",
      "const ARBITRARY = 'rounded-[3px] [border-radius:6px]';",
    ].join('\n');
    expect(inertRadii('fixture.tsx', code)).toEqual([]);
  });
});

describe('square corners (R1)', () => {
  const root = join(__dirname, '..', '..');
  const files = sourceFiles(root);
  const offences = files.flatMap((file) => {
    const path = relative(root, file);
    return offRule(readFileSync(file, 'utf8')).map((o) => `${path}:${o.line} ${o.what}`);
  });
  const key = (offence: string) => offence.split(' ')[0];

  it('resolves every radius step of the Tailwind theme to 0 but full', () => {
    const { borderRadius } = resolveConfig(tailwindConfig).theme as {
      borderRadius: Record<string, string>;
    };
    expect(borderRadius).toEqual({
      none: '0',
      sm: '0',
      DEFAULT: '0',
      md: '0',
      lg: '0',
      xl: '0',
      '2xl': '0',
      '3xl': '0',
      full: '9999px',
    });
  });

  it('declares no radius in globals.css (the well is square)', () => {
    expect(cssRadii(readFileSync(join(root, 'app', 'globals.css'), 'utf8'))).toEqual([]);
  });

  it('scans the page source', () => {
    expect(files.length).toBeGreaterThan(250);
  });

  it('writes no inert radius step and no radius property in a class list (lane E3)', () => {
    const inert = files.flatMap((file) => {
      const path = relative(root, file);
      return inertRadii(path, readFileSync(file, 'utf8')).map((o) => `${path}:${o.line} ${o.what}`);
    });
    expect(inert, `${inert.length} inert radii in page code`).toEqual([]);
  });

  it('rounds nothing but a small mark, and never by an arbitrary or inline radius', () => {
    expect(offences.filter((o) => !(key(o) in ALLOWLIST))).toEqual([]);
  });

  it('keeps the allowlist reasoned and current', () => {
    for (const [site, reason] of Object.entries(ALLOWLIST)) {
      expect(reason.trim().length, site).toBeGreaterThan(10);
      expect(offences.map(key), site).toContain(site);
    }
  });
});
