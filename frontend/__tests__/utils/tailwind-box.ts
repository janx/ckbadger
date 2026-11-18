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
 * a part whose width is the room it is given (`w-full`, `flex-1`), a grid, a value it cannot convert.
 */

type Theme = Record<string, Record<string, unknown>>;

const theme = resolveConfig(tailwindConfig).theme as unknown as Theme;

/** The root font size the rem values are relative to (no rule in app/globals.css changes it). */
const ROOT_PX = 16;

/** A CSS length in px; `em` against `em` px. */
function lengthPx(value: string, em?: number): number {
  const v = value.trim();
  if (v === '0') return 0;
  const m = /^(-?\d*\.?\d+)(px|rem|em)$/.exec(v);
  if (!m) throw new Error(`tailwind-box: cannot read the length "${value}"`);
  const n = parseFloat(m[1]);
  if (m[2] === 'px') return n;
  if (m[2] === 'rem') return n * ROOT_PX;
  if (em === undefined) throw new Error(`tailwind-box: "${value}" needs a font size`);
  return n * em;
}

/** The breakpoints of Tailwind's theme, smallest first. */
export const SCREENS: readonly (readonly [string, number])[] = Object.entries(theme.screens)
  .map(([name, v]) => [name, lengthPx(String(v))] as const)
  .sort((a, b) => a[1] - b[1]);

/**
 * The page's content width at a viewport: Tailwind's `container` (tailwind.config.ts) — the
 * widest of its screens the viewport reaches, the full width below the first — less its padding
 * at the viewport, each side.
 */
export function containerContentPx(viewport: number): number {
  const container = theme.container as {
    screens: Record<string, string>;
    padding: Record<string, string>;
  };
  const screen = (name: string) => {
    const at = SCREENS.find(([n]) => n === name);
    if (!at) throw new Error(`tailwind-box: no screen ${name}`);
    return at[1];
  };
  const widths = Object.values(container.screens).map((v) => lengthPx(v));
  const reached = widths.filter((w) => w <= viewport);
  const width = reached.length ? Math.max(...reached) : viewport;
  let padding = 0;
  let level = -1;
  for (const [name, value] of Object.entries(container.padding)) {
    const at = name === 'DEFAULT' ? 0 : screen(name);
    if (at <= viewport && at > level) {
      level = at;
      padding = lengthPx(value);
    }
  }
  return width - 2 * padding;
}

/** A class split into its variants and its utility, outside brackets (`md:pr-9`, `text-[a:b]`). */
function splitVariants(cls: string): { variants: string[]; utility: string } {
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
  return { variants: parts, utility: cls.slice(start).replace(/^!/, '') };
}

/**
 * Where a class applies at a viewport: null when it does not (a state such as `hover:`, or a
 * breakpoint the viewport is under), else its level, the breakpoint it starts at (0 for none), by
 * which a later breakpoint's class wins.
 */
function levelAt(variants: readonly string[], viewport: number): number | null {
  let level = 0;
  for (const v of variants) {
    const max = /^max-(.+)$/.exec(v);
    const screen = SCREENS.find(([name]) => name === (max ? max[1] : v));
    if (!screen) return null;
    if (max) {
      if (viewport >= screen[1]) return null;
    } else {
      if (viewport < screen[1]) return null;
      level = Math.max(level, screen[1]);
    }
  }
  return level;
}

/** A length, or one of the keywords a width or a margin may take. */
const LENGTH_OR_KEYWORD =
  /^(-?\d*\.?\d+(px|rem|em)|0|auto|fit-content|max-content|min-content|100%|100vw)$/;

/**
 * A theme value of a key, or an arbitrary `[…]` value, as a string; undefined when neither or when
 * it is no length (`border-[color:…]`, `text-[var(--c-text)]` are colours, not sizes).
 */
function themeValue(scale: string, key: string): string | undefined {
  const arbitrary = /^\[(.+)\]$/.exec(key);
  let v: string | undefined;
  if (arbitrary) v = arbitrary[1].replace(/^length:/, '').replace(/_/g, ' ');
  else {
    const t = theme[scale]?.[key];
    if (t !== undefined) v = Array.isArray(t) ? String(t[0]) : String(t);
  }
  if (v === undefined) return undefined;
  if (!LENGTH_OR_KEYWORD.test(v.trim())) {
    // an arbitrary value of a size scale that is not a length is a colour or a calc: refuse what
    // could be a size the reader cannot read, pass what is plainly not one
    if (arbitrary && /\d(px|rem|em)|calc|clamp|min\(|max\(/.test(v) && scale !== 'fontSize')
      throw new Error(`tailwind-box: cannot read [${arbitrary[1]}] as a ${scale}`);
    return undefined;
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
  | 'flexible';

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

/** The declarations of one Tailwind utility that bear on a row's width ([] for the rest). */
function utilityDecls(utility: string): Decl[] {
  if (utility in DISPLAYS) return [{ prop: 'display', value: DISPLAYS[utility], rank: 0 }];
  if (utility === 'sr-only' || utility === 'absolute' || utility === 'fixed')
    return [{ prop: 'position', value: 'absolute', rank: 0 }];
  if (['relative', 'static', 'sticky', 'not-sr-only'].includes(utility))
    return [{ prop: 'position', value: 'static', rank: 0 }];
  if (['flex-1', 'grow', 'flex-grow', 'flex-auto', 'w-full', 'w-screen'].includes(utility))
    return [{ prop: 'flexible', value: utility, rank: 0 }];
  let m = /^(p|px|pl|ps|pr|pe)-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('padding', m[2]);
    if (v === undefined) return [];
    const sides = SIDES[m[1].slice(1)];
    return sides.map((side) => ({
      prop: `padding-${side}` as Prop,
      value: v,
      rank: m![1].length === 1 ? 0 : m![1] === 'px' ? 1 : 2,
    }));
  }
  m = /^(-?)(m|mx|ml|ms|mr|me)-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('margin', m[3]);
    if (v === undefined) return [];
    const sides = SIDES[m[2].slice(1)];
    return sides.map((side) => ({
      prop: `margin-${side}` as Prop,
      value: v === 'auto' ? 'auto' : `${m![1]}${v}`,
      rank: m![2].length === 1 ? 0 : m![2] === 'mx' ? 1 : 2,
    }));
  }
  m = /^gap(-x)?-(.+)$/.exec(utility);
  if (m && !utility.startsWith('gap-y-')) {
    const v = themeValue('gap', m[2]);
    if (v === undefined) return [];
    return [{ prop: 'column-gap', value: v, rank: m[1] ? 1 : 0 }];
  }
  m = /^space-x-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('space', m[1]);
    if (v === undefined) return [];
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
  m = /^w-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('width', m[1]);
    if (v === undefined) return [];
    return [{ prop: 'width', value: v, rank: 0 }];
  }
  m = /^min-w-(.+)$/.exec(utility);
  if (m) {
    const v = themeValue('minWidth', m[1]);
    if (v === undefined) return [];
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
    const v = themeValue('letterSpacing', m[1]);
    if (v === undefined) return [];
    return [{ prop: 'letter-spacing', value: v, rank: 0 }];
  }
  if (/^(divide-x|indent-|basis-|columns-|grid-cols-|max-w-|inline-size)/.test(utility))
    return [{ prop: 'flexible', value: `unread:${utility}`, rank: 0 }];
  return [];
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
};
const CSS_HORIZONTAL =
  /^(padding|padding-inline.*|margin|margin-inline.*|border-left|border-right|border-inline.*|border-width|max-width|gap|font|word-spacing|text-indent|inline-size)$/;

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
};
const INLINE_REFUSED =
  /^(padding|margin|border|border-width|gap|font|max-width|padding-inline.*|margin-inline.*|border-inline.*)$/;

function ownDecls(el: Element, viewport: number): Map<Prop, string> {
  const won = new Map<Prop, { value: string; level: number; rank: number; cls: string }>();
  const fromCss = new Map<Prop, string>();
  for (const cls of Array.from(el.classList)) {
    const { variants, utility } = splitVariants(cls);
    const level = levelAt(variants, viewport);
    if (level === null) continue;
    const decls = utilityDecls(utility);
    if (decls.length === 0 && variants.length === 0) {
      for (const d of cssDecls(cls)) {
        if (fromCss.has(d.prop) && fromCss.get(d.prop) !== d.value)
          throw new Error(`tailwind-box: two stylesheet classes set ${d.prop} on <${el.tagName}>`);
        fromCss.set(d.prop, d.value);
      }
      continue;
    }
    for (const d of decls) {
      const before = won.get(d.prop);
      if (before && before.level === level && before.rank === d.rank && before.value !== d.value)
        throw new Error(
          `tailwind-box: ${before.cls} and ${cls} both set ${d.prop} at one breakpoint on <${el.tagName.toLowerCase()} class="${el.className}">`
        );
      if (!before || level > before.level || (level === before.level && d.rank >= before.rank))
        won.set(d.prop, { value: d.value, level, rank: d.rank, cls });
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
