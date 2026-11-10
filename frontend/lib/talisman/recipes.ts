/**
 * The three seals of the rubbing × Song print theme (decided 2026-11-10; the recipes are the
 * design board's `boards/sealfu.js` P[2], P[7], P[5]):
 *
 * - PAGE — design 3, "the core (符胆)": three dots for the head, five compartments with the
 *   heavier core in the middle, the double knot; red text in a thin frame. Object pages, ~150 px.
 * - ROW — design 8, "white-text, square-ish", **folded from PAGE**: the row seal takes the page
 *   seal's head, its first two compartments (the same figures from the same bits, the same
 *   sizes) and its foot, and lays them in a 7 × 11 white-text stone, so a block is recognisably
 *   the same in a list and on its page. Rows, 28 px.
 * - BRUSH — design 6, "the heavy brush": the painted talisman, no stone, three compartments,
 *   the brush half again as wide. The home page and the 404.
 */

import type { TalismanLayout } from '@/lib/talisman/layout';

interface StoneBase {
  /** Space between the stone's edge (or frame) and the lattice, in units. */
  readonly pad: number;
  /** Stroke width over the gap: the lattice pitch is 1 + ratio. */
  readonly ratio: number;
  /** The brush's width in strokes. */
  readonly w: number;
  /** The renderer's defaults for figures that do not set them (the prototype's `compound`). */
  readonly pulse: number;
  readonly bulge: number;
  readonly tail: number;
}

/**
 * How the seal is pressed: `zhu` red text (ink strokes in a frame of ink), `bai` white text (the
 * strokes cut out of an ink stone, drawn in the ground colour), `ink` the brush alone.
 */
export type Stone =
  | (StoneBase & { readonly style: 'zhu'; /** Frame width in strokes. */ readonly framew: number })
  | (StoneBase & { readonly style: 'bai' | 'ink' });

/**
 * How the strokes are written: `prototype` is the design prototype's markup to the byte (every
 * outline sample, absolute, one decimal); `compact` keeps the points that matter at the drawn size
 * (within `tolerancePx` of the outline), relative and shortest (`encode.ts`), for seals a page
 * draws many times.
 */
export type Encoding =
  | { readonly kind: 'prototype' }
  | { readonly kind: 'compact'; readonly tolerancePx: number };

/** A seal drawn from its own layout. */
export interface DrawnRecipe {
  readonly kind: 'drawn';
  readonly name: 'page' | 'brush';
  readonly layout: TalismanLayout;
  readonly stone: Stone;
  readonly encoding: Encoding;
}

/** A seal folded from another: the source's head, first `keep` compartments and foot. */
export interface FoldRecipe {
  readonly kind: 'fold';
  readonly name: 'row';
  readonly of: DrawnRecipe;
  readonly keep: number;
  /** The folded seal's field in lattice units. */
  readonly W: number;
  readonly H: number;
  readonly stone: Stone;
  readonly encoding: Encoding;
}

export type Recipe = DrawnRecipe | FoldRecipe;
export type RecipeName = Recipe['name'];

const RENDER_DEFAULTS = { pulse: 0.22, bulge: 0.14, tail: 1.2 } as const;

/** P[2] "core": tal { head:'dots3', foot:'doubleKnot', core:true, bands:[5,5], bandH:[2,2,3,2,2] } */
export const PAGE: DrawnRecipe = {
  kind: 'drawn',
  name: 'page',
  layout: {
    W: 6,
    H: 17,
    head: 'dots3',
    foot: 'doubleKnot',
    core: true,
    bands: 5,
    bandH: [2, 2, 3, 2, 2],
  },
  stone: { style: 'zhu', framew: 2, pad: 1.3, ratio: 0.58, w: 0.82, ...RENDER_DEFAULTS },
  encoding: { kind: 'prototype' },
};

/**
 * P[7] "white": the stone (bai, pad 1.1, ratio 0.66, w 0.9) and the field (7 × 11) are design 8's.
 * Design 8's own head, foot and bands (bars3, knotSword, three compartments of 2 : 2 : 2) give way
 * to the fold: the head, the first two compartments and the foot are PAGE's.
 */
export const ROW: FoldRecipe = {
  kind: 'fold',
  name: 'row',
  of: PAGE,
  keep: 2,
  W: 7,
  H: 11,
  stone: { style: 'bai', pad: 1.1, ratio: 0.66, w: 0.9, ...RENDER_DEFAULTS },
  // 20 rows to a list: the compact strokes keep a 28 px seal under 4 KB (the prototype's ~10 KB)
  encoding: { kind: 'compact', tolerancePx: 0.2 },
};

/** P[5] "heavy": tal { head:'bars3', foot:'knotSword', bands:[3,3], bandH:[3,3,3], W:6, H:16 } */
export const BRUSH: DrawnRecipe = {
  kind: 'drawn',
  name: 'brush',
  layout: {
    W: 6,
    H: 16,
    head: 'bars3',
    foot: 'knotSword',
    core: false,
    bands: 3,
    bandH: [3, 3, 3],
  },
  stone: { style: 'ink', pad: 0.8, ratio: 0.62, w: 1.25, ...RENDER_DEFAULTS },
  encoding: { kind: 'prototype' },
};

export const RECIPES: Readonly<Record<RecipeName, Recipe>> = { page: PAGE, row: ROW, brush: BRUSH };
