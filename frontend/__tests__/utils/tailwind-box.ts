import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import resolveConfig from 'tailwindcss/resolveConfig';
import tailwindConfig from '@/tailwind.config';
import { MONO_ADVANCE_EM } from '@/lib/type-metrics';

/**
 * An element's width in a row as its classes and its inline style draw it, read the way the browser
 * reads them (readability review 2026-11-18, review-fb-tests finding 2: a budget checked against a
 * copy of its own constants guards nothing, so a class could change while every test stayed green).
 *
 * Its sources are the ones the browser has, never a module that budgets a part: Tailwind's resolved
 * theme (tailwind.config.ts through tailwindcss's own `resolveConfig`: the spacing scale, the border
 * widths, the letter spacings, the breakpoints, the container and the type scale), the rules
 * app/globals.css writes for a class of its own, and the element's inline style, which the cascade
 * puts over every class. A glyph is the face's advance, 0.6 em (lib/type-metrics.ts, pinned against
 * the woff2 files), plus the tracking it inherits. A test that compares this reading with a budget
 * compares two independent computations of one width.
 *
 * jsdom lays nothing out; this is the arithmetic of a row that does not wrap, at max-content: a flex
 * container is its in-flow items with its column gap between each two; any other box its inline
 * content on one line, a block child as wide as its widest line; an element with a width of its own
 * is that width; each adds its padding and border, and its parent its margins (`auto` adding
 * nothing). What is not in the flow takes no room: `hidden` at the viewport, `sr-only`, `absolute`,
 * `fixed`. What it cannot read it refuses, so a change it does not understand fails loudly instead
 * of measuring nothing: two classes setting one property at one breakpoint, a stylesheet class and
 * a utility setting one property, a horizontal declaration of a stylesheet class it does not read,
 * a part whose width is the room it is given (`w-full`, `flex-1`, `container`), a grid, a value it
 * cannot convert (a `var()`, an unknown key). Every class is placed (review fb-tests SHOULD-2): a
 * utility it reads, one it knows sets no width (`NO_LAYOUT`), or a class of app/globals.css, else
 * it is refused by name; every variant is placed: a screen or an arbitrary breakpoint at its
 * width, a structural one (`first:`, `last:`) where the element is what it names, a state or a
 * pseudo-element box only for a class that sets no width; any other is refused by name.
 */

type Theme = Record<string, Record<string, unknown>>;

const theme = resolveConfig(tailwindConfig).theme as unknown as Theme;

/** The root font size the rem values are relative to (no rule in app/globals.css changes it). */
const ROOT_PX = 16;

/** A CSS length in px; `em` against `em` px, `ch` against its "0", the face's 0.6 em. */
function lengthPx(value: string, em?: number): number {
  const v = value.trim();
  if (v === '0') return 0;
  const m = /^(-?\d*\.?\d+)(px|rem|em|ch)$/.exec(v);
  if (!m) throw new Error(`tailwind-box: cannot read the length "${value}"`);
  const n = parseFloat(m[1]);
  if (m[2] === 'px') return n;
  if (m[2] === 'rem') return n * ROOT_PX;
  if (em === undefined) throw new Error(`tailwind-box: "${value}" needs a font size`);
  return m[2] === 'ch' ? n * MONO_ADVANCE_EM * em : n * em;
}

/** The breakpoints of Tailwind's theme, smallest first. */
export const SCREENS: readonly (readonly [string, number])[] = Object.entries(theme.screens)
  .map(([name, v]) => [name, lengthPx(String(v))] as const)
  .sort((a, b) => a[1] - b[1]);

const container = theme.container as {
  screens: Record<string, string>;
  padding: Record<string, string>;
};

/** Tailwind's `container`'s padding at a viewport, each side (tailwind.config.ts). */
function containerPaddingPx(viewport: number): number {
  const screen = (name: string) => {
    const at = SCREENS.find(([n]) => n === name);
    if (!at) throw new Error(`tailwind-box: no screen ${name}`);
    return at[1];
  };
  let padding = 0;
  let level = -1;
  for (const [name, value] of Object.entries(container.padding)) {
    const at = name === 'DEFAULT' ? 0 : screen(name);
    if (at <= viewport && at > level) {
      level = at;
      padding = lengthPx(value);
    }
  }
  return padding;
}

/**
 * The page's content width at a viewport: Tailwind's `container` (tailwind.config.ts) — the
 * widest of its screens the viewport reaches, the full width below the first — less its padding
 * at the viewport, each side.
 */
export function containerContentPx(viewport: number): number {
  const widths = Object.values(container.screens).map((v) => lengthPx(v));
  const reached = widths.filter((w) => w <= viewport);
  const width = reached.length ? Math.max(...reached) : viewport;
  return width - 2 * containerPaddingPx(viewport);
}

/** A class split into its variants and its utility, outside brackets (`md:pr-9`, `text-[a:b]`). */
function splitVariants(cls: string): { variants: string[]; utility: string; important: boolean } {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < cls.length; i++) {
    if (cls[i] === '[') depth++;
    else if (cls[i] === ']') depth--;
    else if (cls[i] === ':' && depth === 0) {
      parts.push(cls.slice(start, i));
      start = i + 1;
    }
  }
  const utility = cls.slice(start);
  return {
    variants: parts,
    utility: utility.replace(/^!/, ''),
    important: utility.startsWith('!'),
  };
}

/** A state variant: the class is drawn while the element (or its group, its peer) is in it. */
const STATE =
  /^(hover|focus|focus-visible|focus-within|active|visited|disabled|enabled|checked|open|motion-safe|motion-reduce|dark|print|contrast-more|(group|peer)-[a-z-]+(\/[\w-]+)?|(group-|peer-)?(aria|data)-.+)$/;
/** A pseudo-element variant: the class sets a box of its own beside the element's content. */
const PSEUDO = /^(before|after|placeholder|selection|marker|file|first-letter|first-line)$/;

/** Whether an element is what a structural variant names, among its parent's elements. */
function structural(variant: string, el: Element): boolean | null {
  const siblings = el.parentElement ? Array.from(el.parentElement.children) : [el];
  const i = siblings.indexOf(el);
  switch (variant) {
    case 'first':
      return i === 0;
    case 'last':
      return i === siblings.length - 1;
    case 'only':
      return siblings.length === 1;
    case 'odd':
      return i % 2 === 0;
    case 'even':
      return i % 2 === 1;
    default:
      return null;
  }
}

/**
 * Where a class applies at a viewport (review fb-tests SHOULD-2: what the reader cannot read it
 * refuses): its level, the breakpoint it starts at (0 for none), by which a later breakpoint's
 * class wins; null where it does not apply — a breakpoint the viewport is under, a structural
 * variant the element is not (`last:` on a cell that is not the last), a state at rest or a pseudo
 * box out of the flow, when what the class sets bears on no width. A state or an in-flow pseudo box
 * whose class bears on a width, and any variant it does not know (`*:`, `[&>a]:`), are refused by
 * name.
 */
function levelAt(
  cls: string,
  variants: readonly string[],
  viewport: number,
  el: Element,
  bearsOnWidth: () => boolean
): number | null {
  let level = 0;
  let applies = true;
  for (const v of variants) {
    const max = /^max-(.+)$/.exec(v);
    const screen = SCREENS.find(([name]) => name === (max ? max[1] : v));
    const arbitrary = /^(min|max)-\[(\d+)px\]$/.exec(v);
    if (screen) {
      if (max ? viewport >= screen[1] : viewport < screen[1]) applies = false;
      else if (!max) level = Math.max(level, screen[1]);
    } else if (arbitrary) {
      const at = Number(arbitrary[2]);
      if (arbitrary[1] === 'min' ? viewport < at : viewport >= at) applies = false;
      else if (arbitrary[1] === 'min') level = Math.max(level, at);
    } else if (structural(v, el) !== null) {
      // Tailwind writes a pseudo-class variant after the plain utilities and before the screens:
      // over a plain class at its breakpoint, under the next breakpoint
      if (!structural(v, el)) applies = false;
      else level += 0.5;
    } else if (STATE.test(v)) {
      if (bearsOnWidth())
        throw new Error(
          `tailwind-box: ${cls} changes a width under the state ${v}:, which the reader cannot place`
        );
      applies = false;
    } else if (PSEUDO.test(v)) {
      const outOfFlow = ['absolute', 'fixed'].some((p) => el.classList.contains(`${v}:${p}`));
      if (!outOfFlow && bearsOnWidth())
        throw new Error(`tailwind-box: ${cls} sets a width of the ${v} box, which is not read`);
      applies = false;
    } else throw new Error(`tailwind-box: cannot read the variant ${v}: of ${cls}`);
  }
  return applies ? level : null;
}

/** A length, or one of the keywords a width or a margin may take. */
const LENGTH_OR_KEYWORD =
  /^(-?\d*\.?\d+(px|rem|em|ch)|0|auto|fit-content|max-content|min-content|100%|100vw)$/;

/**
 * A theme value of a key, or an arbitrary `[…]` value, as a string. Undefined for a colour: the
 * type and border scales share their prefix with the colours (`text-jade`, `border-row-rule`,
 * `border-[color:…]`, `text-[var(--c-text)]`). Any other value of a size scale it cannot read is
 * refused: an unknown key, a `var()` (review fb-tests SHOULD-2: `px-[var(--gap)]` read as no
 * padding), a percentage, a calc.
 */
function themeValue(scale: string, key: string): string | undefined {
  const colourShared = scale === 'fontSize' || scale === 'borderWidth';
  const arbitrary = /^\[(.+)\]$/.exec(key);
  let v: string | undefined;
  if (arbitrary) {
    if (/^color:/.test(arbitrary[1])) return undefined;
    v = arbitrary[1].replace(/^length:/, '').replace(/_/g, ' ');
    if (/var\(/.test(v) && !(scale === 'fontSize' && !/^length:/.test(arbitrary[1])))
      throw new Error(
        `tailwind-box: cannot read [${arbitrary[1]}] as a ${scale}: a var() has no value here`
      );
  } else {
    const t = theme[scale]?.[key];
    if (t !== undefined) v = Array.isArray(t) ? String(t[0]) : String(t);
  }
  if (v === undefined) {
    if (colourShared) return undefined;
    throw new Error(`tailwind-box: no ${scale} ${key} in the theme`);
  }
  if (!LENGTH_OR_KEYWORD.test(v.trim())) {
    // a value of the type scale that is not a length is a colour; any other is refused
    if (scale === 'fontSize') return undefined;
    throw new Error(
      `tailwind-box: cannot read ${arbitrary ? `[${arbitrary[1]}]` : key} as a ${scale}`
    );
  }
  return v.trim();
}

type Side = 'left' | 'right';
type Prop =
  | 'display'
  | 'position'
  | 'padding-left'
  | 'padding-right'
  | 'margin-left'
  | 'margin-right'
  | 'border-left-width'
  | 'border-right-width'
  | 'border-style'
  | 'column-gap'
  | 'space-x'
  | 'width'
  | 'min-width'
  | 'font-size'
  | 'letter-spacing'
  | 'flexible'
  | 'white-space'
  | 'flex-wrap'
  | 'overflow-wrap'
  | 'word-break'
  | 'flex-direction'
  | 'flex-shrink';

/** One declaration a class makes: the property, its value, and how specific the class is for it. */
interface Decl {
  prop: Prop;
  value: string;
  /** `p` < `px` < `pl` for padding-left; `border` < `border-x` < `border-l`; `gap` < `gap-x`. */
  rank: number;
}

const DISPLAYS: Record<string, string> = {
  hidden: 'none',
  block: 'block',
  'inline-block': 'inline-block',
  inline: 'inline',
  flex: 'flex',
  'inline-flex': 'inline-flex',
  grid: 'grid',
  'inline-grid': 'inline-grid',
  contents: 'contents',
  table: 'table',
};

const SIDES: Record<string, Side[]> = {
  '': ['left', 'right'],
  x: ['left', 'right'],
  l: ['left'],
  s: ['left'],
  r: ['right'],
  e: ['right'],
};

/**
 * Utilities that set nothing a row's width depends on (review fb-tests SHOULD-2: a class the
 * reader does not classify is refused, not read as nothing): colour and paint, the weight and case
 * of a monospaced face (every glyph 0.6 em), the vertical axis, alignment and placement across a
 * line that leave its widths alone, positions (an offset moves a box where it stands or out of the
 * flow), motion, interaction, transforms (drawn, not laid out), overflow and the line-breaking
 * utilities read elsewhere (`wrapsAt`).
 */
const NO_LAYOUT: readonly RegExp[] = [
  /^(bg|fill|stroke|from|via|to|accent|caret)-/,
  /^(shadow|ring|ring-offset|outline|decoration)(-|$)/,
  /^(underline|no-underline|overline|line-through)$/,
  /^underline-offset-/,
  /^font-(thin|extralight|light|normal|medium|semibold|bold|extrabold|black|mono|sans|serif)$/,
  /^(italic|not-italic|uppercase|lowercase|capitalize|normal-case|antialiased|subpixel-antialiased)$/,
  /^(tabular-nums|lining-nums|oldstyle-nums|proportional-nums|ordinal|slashed-zero|normal-nums)$/,
  /^leading-/,
  /^text-(left|center|right|justify|start|end|ellipsis|clip|balance|pretty)$/,
  /^(align|list|hyphens)-/,
  /^(items|justify|content|self|place|order)-/,
  // a grid item's track (a grid is refused where its width is read)
  /^(col|row)-(start|end|span)-/,
  /^(col|row)-auto$/,
  /^(grow-0|flex-row)$/,
  /^-?(h|min-h|max-h|py|pt|pb|my|mt|mb|gap-y|space-y|top|bottom|inset-y)-/,
  /^-?(left|right|inset|inset-x|start|end|z)-/,
  /^(opacity|transition|duration|ease|delay|animate|cursor|select|pointer-events|touch|scroll|snap|will-change|resize|appearance)(-|$)/,
  /^rounded(-|$)/,
  /^(visible|invisible|collapse|isolate|transform|transform-gpu|transform-none)$/,
  /^-?(rotate|scale|scale-x|scale-y|translate-x|translate-y|skew-x|skew-y|origin)-/,
  /^(filter|blur|brightness|contrast|grayscale|hue-rotate|invert|saturate|sepia|drop-shadow|backdrop|mix-blend|bg-blend)(-|$)/,
  /^(overflow|overflow-x|overflow-y|overscroll|object)-/,
  /^border-(solid|dashed|dotted|double|collapse|separate)$/,
  /^table-(auto|fixed)$/,
  /^(group|peer)(\/[\w-]+)?$/,
  /^content-(none|\[.+\])$/,
  /^\[(clip-path|color|text-decoration[a-z-]*|grid-area|background[a-z-]*|opacity|fill|stroke|mask[a-z-]*|font-variant-numeric|text-underline-offset|cursor|transition[a-z-]*):.+\]$/,
];

/**
 * The declarations of one Tailwind utility that bear on a row's width: [] for one it reads that
 * sets none of them (a colour of the type or border scale), null for one it does not read.
 */
function utilityDecls(utility: string): Decl[] | null {
  if (utility in DISPLAYS) return [{ prop: 'display', value: DISPLAYS[utility], rank: 0 }];
  if (utility === 'flex-col' || utility === 'flex-col-reverse')
    return [{ prop: 'flex-direction', value: 'column', rank: 0 }];
  // Tailwind's container: the full width (a part that takes its room) and the theme's gutters
  if (utility === 'container') return [{ prop: 'flexible', value: 'container', rank: 0 }];
  if (utility === 'text-nowrap') return [{ prop: 'white-space', value: 'nowrap', rank: 0 }];
  if (utility === 'text-wrap') return [{ prop: 'white-space', value: 'normal', rank: 0 }];
  if (utility === 'sr-only' || utility === 'absolute' || utility === 'fixed')
    return [{ prop: 'position', value: 'absolute', rank: 0 }];
  if (['relative', 'static', 'sticky', 'not-sr-only'].includes(utility))
    return [{ prop: 'position', value: 'static', rank: 0 }];
  if (['flex-1', 'grow', 'flex-grow', 'flex-auto', 'w-full', 'w-screen'].includes(utility))
    return [{ prop: 'flexible', value: utility, rank: 0 }];
  // how a line may break: Tailwind's white-space, flex-wrap and word-breaking utilities
  let w = /^whitespace-(normal|nowrap|pre|pre-line|pre-wrap|break-spaces)$/.exec(utility);
  if (w) return [{ prop: 'white-space', value: w[1], rank: 0 }];
  if (utility === 'truncate') return [{ prop: 'white-space', value: 'nowrap', rank: 0 }];
  w = /^flex-(wrap|wrap-reverse|nowrap)$/.exec(utility);
  if (w) return [{ prop: 'flex-wrap', value: w[1], rank: 0 }];
  // whether a flex item may shrink below its max-content width (`flex: none` is `0 0 auto`,
  // `flex-initial` `0 1 auto`): one that cannot is a run a line does not break inside
  if (['shrink-0', 'flex-shrink-0', 'flex-none'].includes(utility))
    return [{ prop: 'flex-shrink', value: '0', rank: 0 }];
  if (['shrink', 'flex-shrink', 'flex-initial'].includes(utility))
    return [{ prop: 'flex-shrink', value: '1', rank: 0 }];
  if (utility === 'break-words') return [{ prop: 'overflow-wrap', value: 'break-word', rank: 0 }];
  if (utility === 'break-all') return [{ prop: 'word-break', value: 'break-all', rank: 0 }];
  if (utility === 'break-keep') return [{ prop: 'word-break', value: 'keep-all', rank: 0 }];
  if (utility === 'break-normal')
    return [
      { prop: 'overflow-wrap', value: 'normal', rank: 0 },
      { prop: 'word-break', value: 'normal', rank: 0 },
    ];
  w = /^\[(white-space|flex-wrap|overflow-wrap|word-break):([a-z-]+)\]$/.exec(utility);
  if (w) return [{ prop: w[1] as Prop, value: w[2], rank: 0 }];
  let m = /^(p|px|pl|ps|pr|pe)-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('padding', m[2])!;
    const sides = SIDES[m[1].slice(1)];
    return sides.map((side) => ({
      prop: `padding-${side}` as Prop,
      value: v,
      rank: m![1].length === 1 ? 0 : m![1] === 'px' ? 1 : 2,
    }));
  }
  m = /^(-?)(m|mx|ml|ms|mr|me)-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('margin', m[3])!;
    const sides = SIDES[m[2].slice(1)];
    return sides.map((side) => ({
      prop: `margin-${side}` as Prop,
      value: v === 'auto' ? 'auto' : `${m![1]}${v}`,
      rank: m![2].length === 1 ? 0 : m![2] === 'mx' ? 1 : 2,
    }));
  }
  m = /^gap(-x)?-(.+)$/.exec(utility);
  if (m && !utility.startsWith('gap-y-')) {
    const v = themeValue('gap', m[2])!;
    return [{ prop: 'column-gap', value: v, rank: m[1] ? 1 : 0 }];
  }
  m = /^space-x-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('space', m[1])!;
    return [{ prop: 'space-x', value: v, rank: 0 }];
  }
  if (utility === 'border-none' || utility === 'border-hidden')
    return [{ prop: 'border-style', value: 'none', rank: 0 }];
  m = /^border(?:-([xyltrbse]))?(?:-(.+))?$/.exec(utility);
  if (m) {
    const key = m[2] ?? 'DEFAULT';
    const v = themeValue('borderWidth', key);
    if (v === undefined) return [];
    const side = m[1] ?? '';
    if (!(side in SIDES)) return [];
    return SIDES[side].map((s) => ({
      prop: `border-${s}-width` as Prop,
      value: v,
      rank: side === '' ? 0 : side === 'x' ? 1 : 2,
    }));
  }
  m = /^(w|size)-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue(m[1] === 'w' ? 'width' : 'size', m[2])!;
    return [{ prop: 'width', value: v, rank: 0 }];
  }
  m = /^min-w-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('minWidth', m[1])!;
    return [{ prop: 'min-width', value: v, rank: 0 }];
  }
  m = /^text-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('fontSize', m[1]);
    if (v === undefined || !/^\d*\.?\d+(px|rem|em)$/.test(v)) return [];
    return [{ prop: 'font-size', value: v, rank: 0 }];
  }
  m = /^tracking-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('letterSpacing', m[1])!;
    return [{ prop: 'letter-spacing', value: v, rank: 0 }];
  }
  if (/^(divide-x|-?indent-|basis-|columns-|grid-cols-|max-w-|inline-size)/.test(utility))
    return [{ prop: 'flexible', value: `unread:${utility}`, rank: 0 }];
  // inline-size containment: the box's width is the room it is given, never its content's
  if (utility === '[contain:inline-size]') return [{ prop: 'flexible', value: utility, rank: 0 }];
  if (NO_LAYOUT.some((r) => r.test(utility))) return [];
  return null;
}

// ------------------------------------------------------------------------------------------------
// The stylesheet's own classes (app/globals.css)

const CSS = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf8').replace(
  /\/\*[\s\S]*?\*\//g,
  ''
);

/** Declarations the reader reads off a stylesheet class, the rest of its horizontal ones refused. */
const CSS_READ: Record<string, Prop> = {
  display: 'display',
  position: 'position',
  width: 'width',
  'min-width': 'min-width',
  'font-size': 'font-size',
  'letter-spacing': 'letter-spacing',
  'padding-left': 'padding-left',
  'padding-right': 'padding-right',
  'margin-left': 'margin-left',
  'margin-right': 'margin-right',
  'border-left-width': 'border-left-width',
  'border-right-width': 'border-right-width',
  'column-gap': 'column-gap',
  'white-space': 'white-space',
  'flex-wrap': 'flex-wrap',
  'overflow-wrap': 'overflow-wrap',
  'word-break': 'word-break',
  'flex-direction': 'flex-direction',
  'flex-shrink': 'flex-shrink',
};
const CSS_HORIZONTAL =
  /^(padding|padding-inline.*|margin|margin-inline.*|border-left|border-right|border-inline.*|border-width|max-width|gap|font|word-spacing|text-indent|inline-size)$/;

/** Whether a class is the stylesheet's own: app/globals.css names it in a selector. */
function STYLESHEET_CLASS(cls: string): boolean {
  const escaped = cls.replace(/[.*+?^${}()|[\]\\/:]/g, '\\$&');
  return new RegExp(`\\.${escaped}(?![\\w-])`).test(CSS);
}

/** The rules of the stylesheet whose selector list names `.cls` alone, with where they stand. */
function cssRules(cls: string): { body: string; inAtRule: string | null }[] {
  const out: { body: string; inAtRule: string | null }[] = [];
  const stack: string[] = [];
  let i = 0;
  let selectorStart = 0;
  while (i < CSS.length) {
    const c = CSS[i];
    if (c === '{') {
      const selector = CSS.slice(selectorStart, i).trim();
      if (selector.startsWith('@')) {
        stack.push(selector);
        selectorStart = i + 1;
      } else {
        const end = CSS.indexOf('}', i);
        const names = selector.split(',').map((s) => s.trim());
        if (names.includes(`.${cls}`)) {
          const at = stack.filter((s) => !s.startsWith('@layer'));
          out.push({ body: CSS.slice(i + 1, end), inAtRule: at.length ? at.join(' ') : null });
        }
        i = end;
        selectorStart = end + 1;
      }
    } else if (c === '}') {
      stack.pop();
      selectorStart = i + 1;
    } else if (c === ';' && stack.length === 0) selectorStart = i + 1;
    i++;
  }
  return out;
}

/**
 * The value a class of app/globals.css sets for a property a line breaks by (white-space,
 * overflow-wrap, word-break), undefined where it sets none; refused where it sets it inside an
 * at-rule (a width or a state the reader cannot place).
 */
export function stylesheetValue(
  cls: string,
  prop: 'white-space' | 'overflow-wrap' | 'word-break'
): string | undefined {
  let value: string | undefined;
  for (const rule of cssRules(cls))
    for (const declaration of rule.body.split(';')) {
      const colon = declaration.indexOf(':');
      if (colon < 0 || declaration.slice(0, colon).trim() !== prop) continue;
      if (rule.inAtRule)
        throw new Error(`tailwind-box: .${cls} sets ${prop} in ${rule.inAtRule}; teach the reader`);
      value = declaration.slice(colon + 1).trim();
    }
  return value;
}

const cssCache = new Map<string, Decl[]>();

/** The declarations a stylesheet class makes that bear on a row's width (read once per class). */
function cssDecls(cls: string): Decl[] {
  const cached = cssCache.get(cls);
  if (cached) return cached;
  const out = readCssDecls(cls);
  cssCache.set(cls, out);
  return out;
}

function readCssDecls(cls: string): Decl[] {
  const out: Decl[] = [];
  for (const rule of cssRules(cls)) {
    for (const declaration of rule.body.split(';')) {
      const colon = declaration.indexOf(':');
      if (colon < 0) continue;
      const prop = declaration.slice(0, colon).trim();
      const value = declaration.slice(colon + 1).trim();
      if (prop === 'border') {
        if (rule.inAtRule)
          throw new Error(`tailwind-box: .${cls} sets its border in ${rule.inAtRule}`);
        const width = value.split(/\s+/).find((t) => /^\d*\.?\d+(px|rem|em)$|^0$/.test(t));
        const none = /\bnone\b/.test(value);
        for (const side of ['left', 'right'] as const)
          out.push({ prop: `border-${side}-width`, value: none ? '0' : (width ?? '3px'), rank: 0 });
        continue;
      }
      if (prop in CSS_READ || CSS_HORIZONTAL.test(prop)) {
        if (rule.inAtRule)
          throw new Error(
            `tailwind-box: .${cls} sets ${prop} in ${rule.inAtRule}; teach the reader`
          );
        if (!(prop in CSS_READ))
          throw new Error(`tailwind-box: .${cls} sets ${prop}, which the reader does not read`);
        out.push({ prop: CSS_READ[prop], value, rank: 0 });
      }
    }
  }
  return out;
}

// ------------------------------------------------------------------------------------------------
// One element's own declarations at a viewport

interface Own {
  display: string;
  inFlow: boolean;
  padding: number;
  border: number;
  margin: number;
  /** The column gap between its in-flow items (a flex container), or its `space-x`. */
  gap: number;
  width: number | null;
  minWidth: number | null;
  /** Its width is the room its row gives it, not its content. */
  flexible: string | null;
  fontPx: number | null;
  letterSpacing: string | null;
}

const DEFAULT_DISPLAY: Record<string, string> = {
  div: 'block',
  nav: 'block',
  header: 'block',
  main: 'block',
  section: 'block',
  form: 'block',
  p: 'block',
  ul: 'block',
  ol: 'block',
  li: 'list-item',
  button: 'inline-block',
  input: 'inline-block',
};

/** Inline style properties over the classes, as the cascade has them. */
const INLINE: Record<string, Prop> = {
  display: 'display',
  position: 'position',
  'padding-left': 'padding-left',
  'padding-right': 'padding-right',
  'margin-left': 'margin-left',
  'margin-right': 'margin-right',
  'border-left-width': 'border-left-width',
  'border-right-width': 'border-right-width',
  'column-gap': 'column-gap',
  width: 'width',
  'min-width': 'min-width',
  'font-size': 'font-size',
  'letter-spacing': 'letter-spacing',
  'white-space': 'white-space',
  'flex-wrap': 'flex-wrap',
  'overflow-wrap': 'overflow-wrap',
  'word-break': 'word-break',
  'flex-direction': 'flex-direction',
  'flex-shrink': 'flex-shrink',
};
const INLINE_REFUSED =
  /^(padding|margin|border|border-width|gap|font|max-width|padding-inline.*|margin-inline.*|border-inline.*)$/;

function ownDecls(el: Element, viewport: number): Map<Prop, string> {
  const won = new Map<
    Prop,
    { value: string; level: number; rank: number; important: boolean; cls: string }
  >();
  const fromCss = new Map<Prop, string>();
  for (const cls of Array.from(el.classList)) {
    const { variants, utility, important } = splitVariants(cls);
    // what the class sets: a utility the reader reads (or knows sets no width), or a class of the
    // stylesheet; anything else is refused by name (review fb-tests SHOULD-2)
    const read = utilityDecls(utility);
    if (read === null && !STYLESHEET_CLASS(utility))
      throw new Error(
        `tailwind-box: cannot read ${cls} on <${el.tagName.toLowerCase()}>: no utility the reader knows and no class of app/globals.css`
      );
    const decls = read ?? cssDecls(utility);
    const level = levelAt(cls, variants, viewport, el, () => decls.length > 0);
    if (level === null) continue;
    if (read === null) {
      if (variants.length)
        throw new Error(`tailwind-box: ${cls}, a stylesheet class under a variant, is not read`);
      for (const d of decls) {
        if (fromCss.has(d.prop) && fromCss.get(d.prop) !== d.value)
          throw new Error(`tailwind-box: two stylesheet classes set ${d.prop} on <${el.tagName}>`);
        fromCss.set(d.prop, d.value);
      }
      continue;
    }
    const withGutters =
      utility === 'container'
        ? [
            ...decls,
            ...(['left', 'right'] as const).map((side) => ({
              prop: `padding-${side}` as Prop,
              value: `${containerPaddingPx(viewport)}px`,
              // under any padding class on the element: Tailwind's components layer
              rank: -1,
            })),
          ]
        : decls;
    for (const d of withGutters) {
      const before = won.get(d.prop);
      // a part that takes its room is so whatever else says it (`w-full max-w-…`)
      if (d.prop === 'flexible' && before) continue;
      // an `!important` class is over every class that is not (`!whitespace-normal` over a cell's
      // nowrap), whatever their breakpoints
      if (before && before.important !== important) {
        if (important) won.set(d.prop, { value: d.value, level, rank: d.rank, important, cls });
        continue;
      }
      if (before && before.level === level && before.rank === d.rank && before.value !== d.value)
        throw new Error(
          `tailwind-box: ${before.cls} and ${cls} both set ${d.prop} at one breakpoint on <${el.tagName.toLowerCase()} class="${el.className}">`
        );
      if (!before || level > before.level || (level === before.level && d.rank >= before.rank))
        won.set(d.prop, { value: d.value, level, rank: d.rank, important, cls });
    }
  }
  const out = new Map<Prop, string>();
  for (const [prop, value] of fromCss) {
    if (won.has(prop))
      throw new Error(
        `tailwind-box: a stylesheet class and ${won.get(prop)!.cls} both set ${prop}`
      );
    out.set(prop, value);
  }
  for (const [prop, { value }] of won) out.set(prop, value);
  // the inline style last: it is over every class
  const style = (el as HTMLElement).style;
  if (style)
    for (let i = 0; i < style.length; i++) {
      const name = style[i];
      if (name in INLINE) out.set(INLINE[name], style.getPropertyValue(name));
      else if (INLINE_REFUSED.test(name))
        throw new Error(`tailwind-box: an inline ${name} the reader does not read`);
    }
  return out;
}

/** The font size an element's text is set at: its own, else its parent's (16 px at the root). */
function fontAt(el: Element | null, viewport: number): number {
  for (let e = el; e; e = e.parentElement) {
    const v = ownDecls(e, viewport).get('font-size');
    if (v !== undefined)
      return lengthPx(v, e.parentElement ? fontAt(e.parentElement, viewport) : ROOT_PX);
  }
  return ROOT_PX;
}

/** The letter spacing an element's text inherits, in px (an em resolved where it is declared). */
function trackingAt(el: Element | null, viewport: number): number {
  for (let e = el; e; e = e.parentElement) {
    const v = ownDecls(e, viewport).get('letter-spacing');
    if (v !== undefined) return v === 'normal' ? 0 : lengthPx(v, fontAt(e, viewport));
  }
  return 0;
}

function own(el: Element, viewport: number): Own {
  const d = ownDecls(el, viewport);
  const font = fontAt(el, viewport);
  const len = (prop: Prop) => {
    const v = d.get(prop);
    return v === undefined || v === 'auto' ? 0 : lengthPx(v, font);
  };
  const borderNone = d.get('border-style') === 'none';
  const width = d.get('width');
  const minWidth = d.get('min-width');
  const fixed = (v: string | undefined) =>
    v !== undefined && /^-?\d*\.?\d+(px|rem|em)$|^0$/.test(v.trim()) ? lengthPx(v, font) : null;
  const flexible = d.get('flexible') ?? null;
  if (
    width !== undefined &&
    fixed(width) === null &&
    !['auto', 'fit-content', 'max-content'].includes(width)
  )
    throw new Error(
      `tailwind-box: width ${width} on <${el.tagName.toLowerCase()}> is not a length`
    );
  return {
    display: d.get('display') ?? DEFAULT_DISPLAY[el.tagName.toLowerCase()] ?? 'inline',
    inFlow: d.get('position') !== 'absolute' && d.get('position') !== 'fixed',
    padding: len('padding-left') + len('padding-right'),
    border: borderNone ? 0 : len('border-left-width') + len('border-right-width'),
    margin: len('margin-left') + len('margin-right'),
    gap: d.has('column-gap') ? len('column-gap') : len('space-x'),
    width: fixed(width),
    minWidth: fixed(minWidth),
    flexible,
    fontPx: d.has('font-size') ? font : null,
    letterSpacing: d.get('letter-spacing') ?? null,
  };
}

// ------------------------------------------------------------------------------------------------
// Widths

/** Whether an element is drawn and in the flow at the viewport (a `hidden` ancestor hides it). */
export function inFlowAt(el: Element, viewport: number): boolean {
  const o = own(el, viewport);
  return o.display !== 'none' && o.inFlow;
}

/** An element's horizontal insets at a viewport: its padding and border, and its margins. */
export function insetsPx(el: Element, viewport: number): { box: number; margin: number } {
  const o = own(el, viewport);
  return { box: o.padding + o.border, margin: o.margin };
}

/** The width of a run of text in an element at a viewport: its glyphs and their tracking. */
export function textRunPx(text: string, parent: Element, viewport: number): number {
  const n = [...text].length;
  return n * (MONO_ADVANCE_EM * fontAt(parent, viewport) + trackingAt(parent, viewport));
}

const isBlockLevel = (display: string) =>
  ['block', 'flex', 'grid', 'table', 'list-item'].includes(display);

/**
 * An element's content at max-content, its insets aside: its in-flow items and the gaps between
 * them in a flex container; else its inline content on one line, a block child as wide as its
 * widest line.
 */
export function contentPx(el: Element, viewport: number): number {
  const o = own(el, viewport);
  const flex = o.display === 'flex' || o.display === 'inline-flex';
  if (o.display === 'grid' || o.display === 'inline-grid')
    throw new Error(`tailwind-box: a grid's width is its tracks; the reader does not read them`);
  const items: number[] = [];
  let line = 0;
  let widest = 0;
  for (const child of Array.from(el.childNodes)) {
    if (child.nodeType === 3) {
      const text = (child.textContent ?? '').replace(/\s+/g, ' ');
      if (text === '') continue;
      if (flex) {
        if (text.trim() === '') continue;
        items.push(textRunPx(text, el, viewport));
      } else line += textRunPx(text, el, viewport);
      continue;
    }
    if (!(child instanceof Element)) continue;
    const w = outerPx(child, viewport);
    if (w === null) continue;
    if (flex) items.push(w);
    else if (isBlockLevel(own(child, viewport).display)) {
      widest = Math.max(widest, line, w);
      line = 0;
    } else line += w;
  }
  // a column of items is as wide as its widest; a row is its items and the gaps between them
  if (flex && ownDecls(el, viewport).get('flex-direction') === 'column')
    return Math.max(0, ...items);
  if (flex) return items.reduce((a, b) => a + b, 0) + Math.max(0, items.length - 1) * o.gap;
  return Math.max(widest, line);
}

/**
 * An element's border box at max-content (null when it takes no room at the viewport): a width of
 * its own, else its content and its insets, at least its min-width. A part whose width is the room
 * it is given has no width of its own to read: measure the room around it instead.
 */
export function boxPx(el: Element, viewport: number): number | null {
  const o = own(el, viewport);
  if (o.display === 'none' || !o.inFlow) return null;
  if (o.display === 'contents') return contentPx(el, viewport);
  if (o.flexible !== null)
    throw new Error(
      `tailwind-box: <${el.tagName.toLowerCase()} class="${el.className}"> takes the room it is given (${o.flexible})`
    );
  // a width applies to a box, not to an inline element that is no flex item (CSS ignores it)
  const parent = el.parentElement ? own(el.parentElement, viewport).display : 'block';
  const sized =
    o.display !== 'inline' ||
    REPLACED.has(el.tagName.toLowerCase()) ||
    ['flex', 'inline-flex', 'grid', 'inline-grid'].includes(parent);
  const box = sized && o.width !== null ? o.width : contentPx(el, viewport) + o.padding + o.border;
  return o.minWidth !== null ? Math.max(box, o.minWidth) : box;
}

/** Elements whose width applies while they are inline: they draw a box of their own. */
const REPLACED = new Set(['svg', 'img', 'input', 'video', 'canvas', 'iframe', 'select']);

/** An element's box and its margins, as its parent lays it (null when it takes no room). */
export function outerPx(el: Element, viewport: number): number | null {
  const box = boxPx(el, viewport);
  return box === null ? null : box + own(el, viewport).margin;
}

/** The column gap between a flex container's items at a viewport. */
export function gapPx(el: Element, viewport: number): number {
  return own(el, viewport).gap;
}

// ------------------------------------------------------------------------------------------------
// How a line may break

/** An element's own display at a viewport (its tag's default where no class sets one). */
export function displayAt(el: Element, viewport: number): string {
  return own(el, viewport).display;
}

/** The value of an inherited property at an element: its own, else its nearest ancestor's. */
function inheritedAt(el: Element, viewport: number, prop: Prop): string | null {
  for (let e: Element | null = el; e; e = e.parentElement) {
    const v = ownDecls(e, viewport).get(prop);
    if (v !== undefined) return v;
  }
  return null;
}

/**
 * Whether the text of an element may wrap at its spaces at a viewport: its white-space, inherited
 * (`normal` at the root), read from the nearest element that sets one.
 */
export function wrapsAt(el: Element, viewport: number): boolean {
  const ws = inheritedAt(el, viewport, 'white-space') ?? 'normal';
  return !['nowrap', 'pre'].includes(ws);
}

/** Whether a flex container lays its items in a column at a viewport (else in a row). */
export function flexColumnAt(el: Element, viewport: number): boolean {
  return ownDecls(el, viewport).get('flex-direction') === 'column';
}

/** Whether a flex item may shrink below its max-content width at a viewport (`flex-shrink`). */
export function shrinksAt(el: Element, viewport: number): boolean {
  return (ownDecls(el, viewport).get('flex-shrink') ?? '1') !== '0';
}

/**
 * How an element's own width is set at a viewport: `auto` (its content's), `capped` (`max-w-full`
 * alone: its content's, never wider than its container) or `set` (a width, or a class that takes
 * or bounds its room otherwise: `w-full`, `flex-1`, another `max-w-…`).
 */
export function widthRuleAt(el: Element, viewport: number): 'auto' | 'capped' | 'set' {
  const d = ownDecls(el, viewport);
  const width = d.get('width');
  if (width !== undefined && width !== 'auto') return 'set';
  const flexible = d.get('flexible');
  if (flexible === undefined) return 'auto';
  return flexible === 'unread:max-w-full' ? 'capped' : 'set';
}

/** An element's own min-width at a viewport, as its class sets it, or null where none does. */
export function minWidthAt(el: Element, viewport: number): string | null {
  return ownDecls(el, viewport).get('min-width') ?? null;
}

/**
 * The classes in effect at a viewport that set an element's width or take or bound its room
 * (`w-…`, `basis-…`, `max-w-…`, `flex-1`, …): more than one is a cascade the reader does not
 * decide (it keeps the first that takes the room).
 */
export function widthClassesAt(el: Element, viewport: number): string[] {
  return Array.from(el.classList).filter((cls) => {
    const { variants, utility } = splitVariants(cls);
    const decls = utilityDecls(utility);
    if (decls === null) return false;
    const sets = decls.some((d) => d.prop === 'width' || d.prop === 'flexible');
    return sets && levelAt(cls, variants, viewport, el, () => true) !== null;
  });
}

/** Whether a flex container lays its items on more than one line at a viewport. */
export function flexWrapsAt(el: Element, viewport: number): boolean {
  const v = ownDecls(el, viewport).get('flex-wrap') ?? 'nowrap';
  return v !== 'nowrap';
}

/**
 * The least height an element's target has at a viewport, as its classes set it (the controls lane
 * C1: a control's hit area, docs/RESPONSIVE_LAYOUT.md "Touch"): its own height (`h-*`, a length:
 * the preflight's border-box puts its padding and border inside it), at least its minimum
 * (`min-h-*`), and the reach of a pseudo box drawn over it (`before:` / `after:` with `absolute`, on
 * an element that is `relative`) standing out of it above and below (`-inset-*`, `-inset-y-*`). It
 * reads no height from content, so an element whose classes set none is refused, as is a value it
 * cannot convert (`h-full`, `h-auto`, a `var()`), a height under a state (`hover:h-8`) or a variant
 * it cannot place, a bound (`max-h-*`), two classes setting one height at one breakpoint, and a
 * pseudo box it does not read (one positioned against an ancestor, an inset on one side, a height).
 */
export function targetHeightPx(el: Element, viewport: number): number {
  const set = new Map<'height' | 'minHeight', { px: number; level: number; cls: string }>();
  const reach = new Map<string, { absolute: boolean; inset: number }>();
  for (const cls of Array.from(el.classList)) {
    const { variants, utility } = splitVariants(cls);
    const pseudo = variants.find((v) => PSEUDO.test(v));
    if (pseudo) {
      const box = reach.get(pseudo) ?? { absolute: false, inset: 0 };
      const inset = /^-inset(-y)?-(.+)$/.exec(utility);
      if (utility === 'absolute') box.absolute = true;
      else if (inset) box.inset = lengthPx(themeValue('inset', inset[2])!);
      else if (/^-?(top|bottom|inset|inset-y)-/.test(utility))
        throw new Error(
          `tailwind-box: ${cls} sets an inset of the ${pseudo} box, which is not read`
        );
      else if (/^-?(h|min-h|max-h|py|pt|pb|my|mt|mb)-/.test(utility))
        throw new Error(
          `tailwind-box: ${cls} sets a height of the ${pseudo} box, which is not read`
        );
      reach.set(pseudo, box);
      continue;
    }
    if (/^max-h-/.test(utility))
      throw new Error(`tailwind-box: ${cls} bounds a height, which is not read`);
    const height = /^(h|min-h)-(.+)$/.exec(utility);
    if (!height) continue;
    const state = variants.find((v) => STATE.test(v));
    if (state) throw new Error(`tailwind-box: ${cls} sets a height under a state ${state}:`);
    const level = levelAt(cls, variants, viewport, el, () => true);
    if (level === null) continue;
    const prop = height[1] === 'h' ? 'height' : 'minHeight';
    const px = lengthPx(themeValue(prop, height[2])!);
    const before = set.get(prop);
    if (before && before.level === level && before.px !== px)
      throw new Error(`tailwind-box: ${before.cls} and ${cls} both set ${prop} at one breakpoint`);
    if (!before || level >= before.level) set.set(prop, { px, level, cls });
  }
  const own = set.get('height')?.px;
  const min = set.get('minHeight')?.px;
  if (own === undefined && min === undefined)
    throw new Error(
      `tailwind-box: <${el.tagName.toLowerCase()} class="${el.className}"> sets no height of its own`
    );
  const box = Math.max(own ?? 0, min ?? 0);
  let target = box;
  for (const [pseudo, { absolute, inset }] of reach) {
    if (!absolute || inset <= 0) continue;
    if (!el.classList.contains('relative'))
      throw new Error(
        `tailwind-box: the ${pseudo} box of <${el.tagName.toLowerCase()}> is placed against an ancestor: the element is not relative`
      );
    target = Math.max(target, box + 2 * inset);
  }
  return target;
}

/**
 * Whether a word of an element's text may break inside itself where it overflows (an inherited
 * `overflow-wrap: anywhere | break-word`, or `word-break: break-all | break-word`).
 */
export function breaksWordsAt(el: Element, viewport: number): boolean {
  const wrap = inheritedAt(el, viewport, 'overflow-wrap') ?? 'normal';
  const word = inheritedAt(el, viewport, 'word-break') ?? 'normal';
  return wrap !== 'normal' || ['break-all', 'break-word'].includes(word);
}
