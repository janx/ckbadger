/**
 * The plan of a seal: everything decided before a stroke is drawn — the field, the head, each
 * compartment's figure and the bits it consumed, where it sits, the dividers, the foot. The
 * renderer (`compound.ts`) only draws a plan.
 *
 * A drawn recipe reads the hash as the prototype's `compound` does: the layout's bits first (one
 * per compartment for its height, one for the foot), then, compartment by compartment, the bits
 * that choose its figure (4 from the 14-figure lexicon, 2 for the core's four) and the bits the
 * figure is drawn from. A figure never follows itself: when the choice repeats the figure above,
 * the next in the list is taken.
 *
 * The fold rule (row seal = design 8 folded from design 3): `plan(hash, ROW)` plans the page seal
 * and keeps its head, its first two compartments — the same `Compartment` objects, so the same
 * figures, bits and sizes — and its foot (the same foot from the same bit), all moved right to
 * centre them in the 7-unit stone. The body hangs under the head exactly as on the page; the foot
 * follows the second compartment as it follows the last one on the page, and its sword runs, as
 * every sword does, to the stone's foot, so the sword is what takes up the rest of the stone.
 */

import { BitReader, valueOf, type BitSlice } from '@/lib/seal/bits';
import type { Pt } from '@/lib/seal/brush';
import type { FigureStroke } from '@/lib/seal/textures';
import { CORE_LEXICON, FIGURES, LEXICON, type FigureName } from '@/lib/talisman/figures';
import { BELOW, footOf, layTalisman, type FootName, type HeadName } from '@/lib/talisman/layout';
import type { DrawnRecipe, FoldRecipe, Recipe, RecipeName } from '@/lib/talisman/recipes';

export interface Compartment {
  readonly figure: FigureName;
  /** The bits that chose the figure (before the no-repeat rule). */
  readonly choice: BitSlice;
  /** The bits the figure is drawn from. */
  readonly bits: BitSlice;
  /** The compartment in lattice units, and the figure's lattice in it (rounded). */
  readonly w: number;
  readonly h: number;
  readonly rw: number;
  readonly rh: number;
  /** 1.35 for the core, 1 otherwise. */
  readonly weight: number;
}

export interface Slot {
  readonly x: number;
  readonly y: number;
}

export interface Plan {
  readonly recipe: RecipeName;
  readonly W: number;
  readonly H: number;
  readonly head: { readonly name: HeadName; readonly strokes: readonly FigureStroke[] };
  /** The bits that set the compartments' heights (the page's, for a fold). */
  readonly heights: BitSlice;
  readonly compartments: readonly Compartment[];
  /** Where compartment i sits: its top-left corner in the field. */
  readonly slots: readonly Slot[];
  readonly dividers: readonly FigureStroke[];
  readonly foot: {
    readonly name: FootName;
    readonly bit: BitSlice;
    readonly strokes: readonly FigureStroke[];
  };
  /** How far into the hash the seal reads: every bit it uses is below this. */
  readonly bitsUsed: number;
}

function planDrawn(hash: string, recipe: DrawnRecipe): Plan {
  const read = new BitReader(hash);
  const laid = layTalisman(recipe.layout, read);
  const compartments: Compartment[] = [];
  for (const box of laid.boxes) {
    const choose: readonly FigureName[] = box.core ? CORE_LEXICON : LEXICON;
    const choice = read.read(Math.ceil(Math.log2(choose.length)));
    let idx = valueOf(choice) % choose.length;
    const above = compartments[compartments.length - 1];
    if (above && choose[idx] === above.figure) idx = (idx + 1) % choose.length;
    const figure = choose[idx];
    const rw = Math.round(box.w),
      rh = Math.round(box.h);
    const bits = read.read(FIGURES[figure].bits(rw, rh));
    compartments.push({
      figure,
      choice,
      bits,
      w: box.w,
      h: box.h,
      rw,
      rh,
      weight: box.core ? 1.35 : 1,
    });
  }
  return {
    recipe: recipe.name,
    W: laid.W,
    H: laid.H,
    head: { name: recipe.layout.head, strokes: laid.head },
    heights: laid.heights,
    compartments,
    slots: laid.boxes.map(({ x, y }) => ({ x, y })),
    dividers: laid.dividers,
    foot: { name: recipe.layout.foot, bit: laid.footBit, strokes: laid.foot },
    bitsUsed: read.at,
  };
}

const shift = (strokes: readonly FigureStroke[], dx: number): FigureStroke[] =>
  strokes.map((s) => ({ ...s, pts: s.pts.map(([x, y]): Pt => [x + dx, y]) }));

function planFold(hash: string, recipe: FoldRecipe): Plan {
  const page = planDrawn(hash, recipe.of);
  const keep = recipe.keep;
  if (!Number.isInteger(keep) || keep < 1 || keep > page.compartments.length)
    throw new Error(
      `talisman: a fold keeps 1–${page.compartments.length} compartments of ${page.recipe}, not ${keep}`
    );
  if (recipe.W < page.W)
    throw new Error(
      `talisman: a ${recipe.W}-unit stone cannot hold the ${page.W}-unit ${page.recipe} seal`
    );
  const dx = (recipe.W - page.W) / 2;
  const compartments = page.compartments.slice(0, keep);
  const slots = page.slots.slice(0, keep).map(({ x, y }) => ({ x: x + dx, y }));
  // the foot follows the last kept compartment as, on the page, it follows the last one
  const tail = compartments[keep - 1];
  const y = slots[keep - 1].y + tail.h + BELOW;
  return {
    recipe: recipe.name,
    W: recipe.W,
    H: recipe.H,
    head: { name: page.head.name, strokes: shift(page.head.strokes, dx) },
    heights: page.heights,
    compartments,
    slots,
    dividers: shift(page.dividers.slice(0, keep - 1), dx),
    foot: {
      name: page.foot.name,
      bit: page.foot.bit,
      strokes: footOf(page.foot.name, recipe.W / 2, y, recipe.H, page.foot.bit.bits[0]),
    },
    bitsUsed: tail.bits.at + tail.bits.bits.length,
  };
}

/** Decide the seal: the hash and the recipe, nothing else. */
export function plan(hash: string, recipe: Recipe): Plan {
  return recipe.kind === 'drawn' ? planDrawn(hash, recipe) : planFold(hash, recipe);
}
