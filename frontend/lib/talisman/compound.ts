/**
 * The seal renderer (the prototype's `compound`): draws a plan with the stone's brush and returns
 * the SVG markup, with the bits it read and the figures it chose.
 *
 * The markup is the generator's own: fixed element names, numbers, the two paints (validated:
 * a hex colour or a `var(--c-…)` palette reference), the tilt (a finite number) and the first ten
 * characters of the hash (validated lowercase hex). Nothing else enters it, so it can be set as
 * inner HTML without a sanitiser.
 */

import {
  f2,
  orderStrokes,
  shape,
  stroke,
  type Outline,
  type Pt,
  type StrokeOptions,
} from '@/lib/talisman/brush';
import { compactCap, compactOutline } from '@/lib/talisman/encode';
import {
  FIGURES,
  type BrushSpec,
  type FigureName,
  type FigureStroke,
} from '@/lib/talisman/figures';
import { plan, type Plan } from '@/lib/talisman/plan';
import type { Recipe, Stone } from '@/lib/talisman/recipes';
import { assertSealHash } from '@/lib/talisman/bits';

export interface SealPaint {
  /** The stone and the red text: frame and strokes (zhu), the stone (bai), the strokes (ink). */
  readonly ink: string;
  /** The ground: the colour the white-text strokes are cut in. */
  readonly ground: string;
  /** Degrees; the prototype turns the whole seal with a CSS transform. */
  readonly tilt?: number;
}

export interface Seal {
  readonly svg: string;
  readonly bitsUsed: number;
  readonly chosen: FigureName[];
}

const PAINT = /^(#[0-9a-f]{6}|var\(--c-[a-z0-9-]+\))$/i;

export function assertPaint(name: string, value: string): void {
  if (typeof value !== 'string' || !PAINT.test(value))
    throw new Error(
      `hash seal: ${name} must be a hex colour or a var(--c-…) palette reference, got ${JSON.stringify(value)}`
    );
}

/** The brush the layout's own strokes (head, dividers, foot) are written with. */
const EXTRA_BRUSH: BrushSpec = {
  curve: 'corner',
  r: 0.2,
  over: 0.1,
  entry: 'point',
  exit: 'taper',
};

/** One decimal is enough at these sizes; it halves the file (the prototype truncates f2's output). */
const oneDecimal = (markup: string): string => markup.replace(/(\d+\.\d)\d+/g, '$1');

/** The outlines of a run of strokes, in the seal's user units. */
function outlines(
  strokes: readonly FigureStroke[],
  brush: BrushSpec,
  X: (x: number) => number,
  Y: (y: number) => number,
  weight: number,
  stone: Stone
): Outline[] {
  const w0 = stone.ratio,
    p = 1 + w0;
  return strokes.map((st) => {
    const P = st.pts.map(([x, y]): Pt => [X(x), Y(y)]);
    const sp: BrushSpec = { ...brush, ...(st.brush ?? {}) };
    let o: StrokeOptions = {
      w: w0 * stone.w * weight * (st.weight ?? 1),
      r: (sp.r ?? 0.3) * p,
      over: sp.over ?? 0.3,
      entry: sp.entry ?? 'point',
      entryLen: 0.6 * p,
      press0: sp.press0 ?? 0.08,
      exit: sp.exit ?? 'taper',
      tail: (sp.tail ?? stone.tail) * p,
      bulge: sp.even ? 0 : (sp.bulge ?? stone.bulge),
      bulgeSigma: sp.bulgeSigma,
      hmul: sp.hmul ?? (sp.even ? 1 : 0.86),
      pulse: sp.even ? 0 : (sp.pulse ?? stone.pulse),
      unit: p,
      curve: sp.curve ?? 'corner',
      tension: sp.tension ?? 4,
      step: p / 4,
      spm: 5,
      qm: 6,
    };
    if (st.horiz === true)
      o = { ...o, entry: 'point', entryLen: 0.4 * p, exit: 'blunt', pulse: 0, curve: 'corner' };
    if (st.horiz === false)
      o = {
        ...o,
        entry: 'round',
        press0: 0.2,
        exit: 'taper',
        tail: 0.7 * p,
        pulse: 0,
        curve: 'corner',
      };
    return stroke(P, o);
  });
}

/** Draw a planned seal at `size` px (its longer side). */
export function renderPlan(
  hash: string,
  pl: Plan,
  recipe: Recipe,
  size: number,
  paint: SealPaint
): Seal {
  assertSealHash(hash);
  assertPaint('ink', paint.ink);
  assertPaint('ground', paint.ground);
  if (!(Number.isFinite(size) && size > 0)) throw new Error(`hash seal: size ${size} px`);
  const tilt = paint.tilt ?? 0;
  if (!Number.isFinite(tilt)) throw new Error(`hash seal: tilt ${tilt}°`);
  const stone = recipe.stone;
  const u = 1,
    w0 = stone.ratio,
    p = u + w0;
  const wf = stone.style === 'zhu' ? w0 * stone.framew : 0;
  const pad = stone.pad * u;
  const W = 2 * wf + 2 * pad + pl.W * p + w0,
    H = 2 * wf + 2 * pad + pl.H * p + w0;
  const ox = wf + pad + w0 / 2,
    oy = wf + pad + w0 / 2;
  const col = stone.style === 'bai' ? paint.ground : paint.ink;
  /* the figures compartment by compartment, then the layout's own strokes (head, dividers, foot);
     each run is written apart, as the prototype writes it */
  const runs: Outline[][] = pl.compartments.map((c, i) => {
    const slot = pl.slots[i];
    const fig = FIGURES[c.figure];
    let strokes = fig.make(c.bits.bits, c.rw, c.rh);
    if (!fig.brush.even) strokes = orderStrokes(strokes);
    const sx = c.w / c.rw,
      sy = c.h / c.rh;
    return outlines(
      strokes,
      fig.brush,
      (x) => ox + (slot.x + x * sx) * p,
      (y) => oy + (slot.y + y * sy) * p,
      c.weight,
      stone
    );
  });
  const extra = [...pl.head.strokes, ...pl.dividers, ...pl.foot.strokes];
  if (extra.length)
    runs.push(
      outlines(
        extra,
        EXTRA_BRUSH,
        (x) => ox + x * p,
        (y) => oy + y * p,
        1,
        stone
      )
    );
  const scale = size / Math.max(W, H);
  let shapes: string;
  if (recipe.encoding.kind === 'prototype')
    shapes = runs.map((run) => oneDecimal(run.map((r) => shape(r, col)).join(''))).join('');
  else {
    const tol = recipe.encoding.tolerancePx / scale;
    const body = runs
      .flat()
      .map((r) => `<path d="${compactOutline(r.polygon, tol)}"/>` + r.caps.map(compactCap).join(''))
      .join('');
    shapes = `<g fill="${col}">${body}</g>`;
  }
  const frame =
    stone.style === 'zhu'
      ? `<rect x="${f2(wf / 2)}" y="${f2(wf / 2)}" width="${f2(W - wf)}" height="${f2(H - wf)}" rx="${f2(0.7 * u)}" fill="none" stroke="${paint.ink}" stroke-width="${f2(wf)}"/>`
      : '';
  const field =
    stone.style === 'bai'
      ? `<rect x="0" y="0" width="${f2(W)}" height="${f2(H)}" rx="${f2(0.4 * u)}" fill="${paint.ink}"/>`
      : '';
  const pxW = Math.round(W * scale),
    pxH = Math.round(H * scale);
  const svg = `<svg width="${pxW}" height="${pxH}" viewBox="0 0 ${f2(W)} ${f2(H)}" role="img" aria-label="Hash seal ${hash.slice(0, 10)}"${tilt ? ` style="transform: rotate(${tilt}deg)"` : ''}>${field}${frame}${shapes}</svg>`;
  return { svg, bitsUsed: pl.bitsUsed, chosen: pl.compartments.map((c) => c.figure) };
}

/** The seal of `hash` by `recipe`, `size` px on its longer side. */
export function compound(hash: string, recipe: Recipe, size: number, paint: SealPaint): Seal {
  return renderPlan(hash, plan(hash, recipe), recipe, size, paint);
}
