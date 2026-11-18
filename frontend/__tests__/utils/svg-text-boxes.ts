import { expect } from 'vitest';
import { lineAscentPx } from '@/lib/chart-type';
import { descentPx, textPx } from '@/lib/type-metrics';

/**
 * Where a figure's SVG text stands, by arithmetic from its own attributes (readability plan
 * 2026-11-18, lane F3, the figures' text budgets). jsdom lays nothing out, so a line of text is
 * measured as the type modules state it:
 *
 * - across, its glyphs at 0.6 em of their own size (`textPx`), from its x by its text-anchor;
 * - down, the tight line it occupies (`tightLinePx`: its size and 3 px of leading), standing on
 *   its baseline: its descent (`descentPx`, 0.3 em) under the baseline, the rest of the line over
 *   it (`lineAscentPx`: 11.4 px at 12 px).
 *
 * Each `<tspan>` with its own x starts a line of its own; a tspan's font size counts for its own
 * glyphs. Every `translate` and `rotate` on the text and its ancestors is applied, so a box is in
 * the svg's user space. A `<title>` inside a text is not drawn and is not counted.
 */

export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface TextLine extends Box {
  text: string;
  /** The largest size a run of the line is set in. */
  size: number;
}

type Matrix = [number, number, number, number, number, number];
const IDENTITY: Matrix = [1, 0, 0, 1, 0, 0];

function multiply(m: Matrix, n: Matrix): Matrix {
  return [
    m[0] * n[0] + m[2] * n[1],
    m[1] * n[0] + m[3] * n[1],
    m[0] * n[2] + m[2] * n[3],
    m[1] * n[2] + m[3] * n[3],
    m[0] * n[4] + m[2] * n[5] + m[4],
    m[1] * n[4] + m[3] * n[5] + m[5],
  ];
}

/** An element's own `transform` attribute as a matrix: translate and rotate, as the figures write them. */
function ownTransform(el: Element): Matrix {
  const attr = el.getAttribute('transform');
  if (!attr) return IDENTITY;
  let m = IDENTITY;
  for (const [, fn, args] of attr.matchAll(/(\w+)\(([^)]*)\)/g)) {
    const v = args
      .split(/[\s,]+/)
      .filter(Boolean)
      .map(Number);
    if (fn === 'translate') m = multiply(m, [1, 0, 0, 1, v[0], v[1] ?? 0]);
    else if (fn === 'rotate' && v.length === 1) {
      const a = (v[0] * Math.PI) / 180;
      const [c, s] = [Math.round(Math.cos(a) * 1e12) / 1e12, Math.round(Math.sin(a) * 1e12) / 1e12];
      m = multiply(m, [c, s, -s, c, 0, 0]);
    } else throw new Error(`svg-text-boxes: transform ${fn}(${args}) is not read`);
  }
  return m;
}

/** The matrix from an element's own space to its svg's user space. */
function toUser(el: Element): Matrix {
  const chain: Element[] = [];
  for (let e: Element | null = el; e && e.tagName.toLowerCase() !== 'svg'; e = e.parentElement)
    chain.unshift(e);
  return chain.reduce((m, e) => multiply(m, ownTransform(e)), IDENTITY);
}

function mapBox(m: Matrix, b: Box): Box {
  const xs: number[] = [];
  const ys: number[] = [];
  for (const [x, y] of [
    [b.left, b.top],
    [b.right, b.top],
    [b.left, b.bottom],
    [b.right, b.bottom],
  ]) {
    xs.push(m[0] * x + m[2] * y + m[4]);
    ys.push(m[1] * x + m[3] * y + m[5]);
  }
  return {
    left: Math.min(...xs),
    top: Math.min(...ys),
    right: Math.max(...xs),
    bottom: Math.max(...ys),
  };
}

function fontSizeOf(el: Element): number {
  for (let e: Element | null = el; e; e = e.parentElement) {
    const v = e.getAttribute('font-size');
    if (v !== null) return Number(v);
  }
  throw new Error(`svg-text-boxes: no font size for "${el.textContent}"`);
}

interface Run {
  text: string;
  size: number;
}
interface Chunk {
  x: number;
  y: number;
  runs: Run[];
}

/** The lines of one `<text>`, each as its box in the svg's user space. */
export function textLines(text: Element): TextLine[] {
  // text-anchor is inherited, as font-size is: a group may set it for its texts
  let anchor = 'start';
  for (let e: Element | null = text; e; e = e.parentElement) {
    const v = e.getAttribute('text-anchor');
    if (v !== null) {
      anchor = v;
      break;
    }
  }
  const chunks: Chunk[] = [
    { x: Number(text.getAttribute('x') ?? 0), y: Number(text.getAttribute('y') ?? 0), runs: [] },
  ];
  const walk = (el: Element) => {
    for (const node of Array.from(el.childNodes)) {
      if (node.nodeType === 3) {
        chunks[chunks.length - 1].runs.push({ text: node.textContent ?? '', size: fontSizeOf(el) });
        continue;
      }
      if (node.nodeType !== 1) continue;
      const child = node as Element;
      const tag = child.tagName.toLowerCase();
      if (tag === 'title') continue;
      if (tag !== 'tspan') throw new Error(`svg-text-boxes: <${tag}> inside a text is not read`);
      const x = child.getAttribute('x');
      const dy = child.getAttribute('dy');
      const y = child.getAttribute('y');
      if (x !== null || y !== null || dy !== null) {
        if (x === null)
          throw new Error(
            `svg-text-boxes: a tspan moved down without its own x ("${child.textContent}")`
          );
        const last = chunks[chunks.length - 1];
        chunks.push({
          x: Number(x),
          y: y !== null ? Number(y) : last.y + Number(dy ?? 0),
          runs: [],
        });
      }
      walk(child);
    }
  };
  walk(text);
  const m = toUser(text);
  // SVG collapses white space: runs of it are one space, and none leads or ends the text.
  const lines: TextLine[] = [];
  chunks.forEach((chunk, i) => {
    const runs = chunk.runs.map((r) => ({ ...r, text: r.text.replace(/\s+/g, ' ') }));
    if (i === 0 && runs.length) runs[0].text = runs[0].text.replace(/^ /, '');
    if (i === chunks.length - 1 && runs.length)
      runs[runs.length - 1].text = runs[runs.length - 1].text.replace(/ $/, '');
    const shown = runs.filter((r) => r.text.length > 0);
    if (!shown.length) return;
    const width = shown.reduce((w, r) => w + textPx(Array.from(r.text).length, r.size), 0);
    const left =
      anchor === 'end' ? chunk.x - width : anchor === 'middle' ? chunk.x - width / 2 : chunk.x;
    const size = Math.max(...shown.map((r) => r.size));
    const box = mapBox(m, {
      left,
      right: left + width,
      top: chunk.y - Math.max(...shown.map((r) => lineAscentPx(r.size))),
      bottom: chunk.y + Math.max(...shown.map((r) => descentPx(r.size))),
    });
    lines.push({ ...box, text: shown.map((r) => r.text).join(''), size });
  });
  return lines;
}

/** Every line of every `<text>` under `root`. */
export function allTextLines(root: ParentNode): TextLine[] {
  return Array.from(root.querySelectorAll('text')).flatMap(textLines);
}

/**
 * The inside of a drawn `<rect>` in the svg's user space: its area less half its stroke, so a word
 * that stands inside it stands clear of its border.
 */
export function rectBox(rect: Element): Box {
  const [x, y, w, h] = ['x', 'y', 'width', 'height'].map((a) => Number(rect.getAttribute(a) ?? 0));
  const stroke = rect.getAttribute('stroke');
  const half = stroke && stroke !== 'none' ? Number(rect.getAttribute('stroke-width') ?? 1) / 2 : 0;
  return mapBox(toUser(rect), {
    left: x + half,
    top: y + half,
    right: x + w - half,
    bottom: y + h - half,
  });
}

/** An svg's own box, from its viewBox. */
export function viewBox(svg: Element): Box {
  const [x, y, w, h] = svg
    .getAttribute('viewBox')!
    .split(/[\s,]+/)
    .map(Number);
  return { left: x, top: y, right: x + w, bottom: y + h };
}

const fmt = (b: Box) =>
  `[${[b.left, b.top, b.right, b.bottom].map((v) => Math.round(v * 100) / 100).join(', ')}]`;

/**
 * Whether a line stands inside a box. Inside a drawn tile or band it touches no edge: it keeps
 * clear on every side (`clear`, the default). Inside the svg, a line set flush with the figure's
 * edge (a value written to the column's right end) stands inside it (`flush`).
 */
export function expectInside(
  line: TextLine,
  box: Box,
  what: string,
  mode: 'clear' | 'flush' = 'clear'
) {
  expect(
    standsInside(line, box, mode),
    `${what}: "${line.text}" ${fmt(line)} is not inside ${fmt(box)}`
  ).toBe(true);
}

/** Whether a line stands inside a box, as `expectInside` holds it. */
export function standsInside(line: Box, box: Box, mode: 'clear' | 'flush' = 'clear'): boolean {
  return mode === 'clear'
    ? line.left > box.left &&
        line.right < box.right &&
        line.top > box.top &&
        line.bottom < box.bottom
    : line.left >= box.left - 1e-9 &&
        line.right <= box.right + 1e-9 &&
        line.top >= box.top - 1e-9 &&
        line.bottom <= box.bottom + 1e-9;
}

/**
 * Whether two lines meet. Each carries its own leading over its glyphs, so two lines stacked a
 * tight line apart stand clear; side by side, they must not touch.
 */
export function meets(a: Box, b: Box): boolean {
  return (
    a.left <= b.right && b.left <= a.right && a.top < b.bottom - 1e-9 && b.top < a.bottom - 1e-9
  );
}

const hundredth = (px: number) => Math.round(px * 100) / 100;

/** Two lines that meet, and how far across they share, px to the hundredth. */
export interface Meeting {
  a: string;
  b: string;
  by: number;
}

/**
 * Every two of `lines` that meet (`meets`), each with how far across the two overlap: the
 * measure a known fault is pinned by, so a test holds it at its size until it is fixed.
 */
export function meetings(lines: readonly TextLine[]): Meeting[] {
  const met: Meeting[] = [];
  lines.forEach((a, i) =>
    lines.slice(i + 1).forEach((b) => {
      if (meets(a, b))
        met.push({
          a: a.text,
          b: b.text,
          by: hundredth(Math.min(a.right, b.right) - Math.max(a.left, b.left)),
        });
    })
  );
  return met;
}

/**
 * How far a line stands outside a box on its farthest side, px to the hundredth; 0 or less when
 * it stands inside it (flush counts as inside).
 */
export function pastPx(line: Box, box: Box): number {
  return hundredth(
    Math.max(
      box.left - line.left,
      line.right - box.right,
      box.top - line.top,
      line.bottom - box.bottom
    )
  );
}

/** No two of `lines` meet; a failure names both lines and their boxes. */
export function expectApart(lines: readonly TextLine[], what: string) {
  const met: string[] = [];
  lines.forEach((a, i) =>
    lines
      .slice(i + 1)
      .forEach((b) => meets(a, b) && met.push(`"${a.text}" ${fmt(a)} meets "${b.text}" ${fmt(b)}`))
  );
  expect(met, what).toEqual([]);
}
