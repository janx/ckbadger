import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { themeDeclarations, type ThemeName } from '@/lib/palette';

/**
 * Reading what a rule of app/globals.css paints in a theme (readability R1, the scan): a rule's
 * declarations, its `var(--…)` put in from the theme's declarations (lib/palette.ts, as
 * tailwind.config.ts emits them), and the stops of a repeating gradient.
 */

export const GLOBALS_CSS = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf8');

/** The body of a class's own rule (`.crt-grain { … }`, at the start of a line). */
export function ruleOf(className: string, css = GLOBALS_CSS): string {
  const m = css.match(new RegExp(`(?:^|\\n)\\.${className}\\s*\\{([^}]*)\\}`));
  if (!m) throw new Error(`globals.css has no rule .${className}`);
  return m[1];
}

/**
 * A declaration of a rule body, or undefined (`mask-image` is not read out of a
 * `-webkit-mask-image`: the name must stand after a space, a `;` or the start).
 */
export function declaration(rule: string, property: string): string | undefined {
  const m = rule.match(new RegExp(`(?:^|[;{\\s])${property}:\\s*([^;]+);`));
  return m?.[1].trim();
}

/** A value with every `var(--…)` put in from the theme's declarations. */
export function inTheme(theme: ThemeName, value: string): string {
  const declared = themeDeclarations(theme);
  return value.replace(/var\((--[\w-]+)\)/g, (_, name: string) => {
    if (!(name in declared)) throw new Error(`${name} is not a ${theme} theme variable`);
    return declared[name];
  });
}

/** The arguments of a CSS function, split at its own commas (not those of an `rgba(…)`). */
export function argumentsOf(value: string, fn: string): string[] {
  const start = value.indexOf(`${fn}(`);
  if (start < 0) throw new Error(`${value} is not a ${fn}()`);
  const out: string[] = [];
  let depth = 0;
  let current = '';
  for (const ch of value.slice(start + fn.length + 1)) {
    if (ch === '(') depth++;
    if (ch === ')') {
      if (depth === 0) break;
      depth--;
    }
    if (ch === ',' && depth === 0) {
      out.push(current.trim());
      current = '';
    } else current += ch;
  }
  out.push(current.trim());
  return out;
}

/** A colour's alpha: `transparent` is 0, `rgba(r, g, b, a)` is a. */
export function alphaOf(colour: string): number {
  if (colour === 'transparent') return 0;
  const m = colour.match(/^rgba\(\d+, \d+, \d+, ([\d.]+)\)$/);
  if (!m) throw new Error(`${colour} is not transparent or rgba(r, g, b, a)`);
  return Number(m[1]);
}

/** The stops of a repeating linear gradient: each colour with the px it stands at. */
export function stopsOf(gradient: string): { colour: string; at: number }[] {
  const [, ...stops] = argumentsOf(gradient, 'repeating-linear-gradient');
  return stops.map((stop) => {
    const m = stop.match(/^(.*\S)\s+(\d+)px$/);
    if (!m) throw new Error(`${stop} is not a colour at a px`);
    return { colour: m[1], at: Number(m[2]) };
  });
}
