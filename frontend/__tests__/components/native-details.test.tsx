import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

// Visual finish V9 (plan 2026-11-04): the browser's own details — the colour scheme, form
// accents, the text selection, the keyboard focus ring and the caret — take the palette's jade
// instead of the platform's blue. The rules are base styles (`@layer base`), so a control whose
// own class string designs its focus (`focus-visible:…` with `outline-none`) keeps that design.

const root = resolve(__dirname, '../..');
const css = readFileSync(join(root, 'app/globals.css'), 'utf-8');

/** The body of `@layer base { … }` blocks, braces balanced. */
function baseLayers(source: string): string[] {
  const bodies: string[] = [];
  const re = /@layer base\s*\{/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(source))) {
    let depth = 1;
    let i = m.index + m[0].length;
    const start = i;
    for (; i < source.length && depth; i++)
      if (source[i] === '{') depth++;
      else if (source[i] === '}') depth--;
    bodies.push(source.slice(start, i - 1));
  }
  return bodies;
}

describe('the native details in globals.css', () => {
  const base = baseLayers(css).join('\n');

  it('declares a dark colour scheme and a jade accent', () => {
    expect(base).toMatch(/:root\s*\{\s*color-scheme: dark;\s*accent-color: var\(--c-jade\);\s*\}/);
  });

  it('selects text in jade', () => {
    expect(base).toMatch(
      /::selection\s*\{\s*background: rgb\(var\(--rgb-jade\) \/ 0\.28\);\s*color: var\(--c-selection-ink\);\s*\}/
    );
  });

  it('rings the keyboard focus in jade, 1 px, 2 px off, with a soft glow', () => {
    expect(base).toMatch(
      /(^|\n)\s*:focus-visible\s*\{\s*outline: 1px solid var\(--c-jade\);\s*outline-offset: 2px;\s*box-shadow: 0 0 6px rgb\(var\(--rgb-jade\) \/ calc\(0\.45 \* var\(--m-glow\)\)\);\s*\}/
    );
  });

  it('draws the caret in jade', () => {
    expect(base).toMatch(/input,\s*textarea\s*\{\s*caret-color: var\(--c-jade\);\s*\}/);
  });

  it('keeps them base styles, so a utility on the element wins', () => {
    // the four rules appear nowhere outside a base layer
    let rest = css;
    for (const body of baseLayers(css)) rest = rest.replace(body, '');
    expect(rest).not.toMatch(
      /::selection|(^|\n)\s*:focus-visible\s*\{|caret-color: var\(--c-jade\)/
    );
    expect(rest).not.toMatch(/color-scheme: dark/);
  });
});

/** Every .ts / .tsx file under a directory. */
function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return sources(p);
    return /\.tsx?$/.test(e.name) ? [p] : [];
  });
}

/** The string literal (quote to quote) around position `at`. */
function enclosingString(text: string, at: number): string {
  let start = at;
  while (start > 0 && !`'"\``.includes(text[start - 1])) start--;
  const quote = text[start - 1];
  let end = at;
  while (end < text.length && text[end] !== quote) end++;
  return text.slice(start, end);
}

describe('outline-none (visual finish V9)', () => {
  it('appears only in a class string that designs its own focus-visible state', () => {
    const sites: string[] = [];
    const bare: string[] = [];
    for (const dir of ['app', 'components'])
      for (const file of sources(join(root, dir))) {
        const text = readFileSync(file, 'utf-8');
        for (const m of text.matchAll(/(?:[\w-]+:)*outline-none/g)) {
          const where = `${relative(root, file)}:${text.slice(0, m.index).split('\n').length}`;
          sites.push(where);
          if (!enclosingString(text, m.index!).includes('focus-visible:')) bare.push(where);
        }
      }
    // the sites that keep it: each carries its own focus-visible style in the same string
    expect(sites.length).toBeGreaterThan(0);
    expect(bare).toEqual([]);
  });
});
