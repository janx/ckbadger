/**
 * The talisman's layout (the prototype's `LAYOUTS.talisman2`, the parts the three recipes use):
 * a head, a body of stacked compartments with a divider between each two, a foot. Everything is
 * in lattice units, x right and y down, in a W × H field.
 *
 * - heads: `dots3` (three dots for the Three Pure Ones) and `bars3` (☰).
 * - feet: `doubleKnot` (two knots turning against each other, the sword under them, hooked by a
 *   bit) and `knotSword` (one knot, turned by a bit, and the hooked sword).
 * - body: `bands` compartments; each one's height is `bandH[i]` or `bandH[i + 1]` by a bit, all
 *   scaled to fill what the head and foot leave; with `core`, the middle one is the core (符胆),
 *   drawn heavier from its own short lexicon.
 *
 * Not ported (no recipe uses them): the crown, thunder and roof heads; the tails, lock-and-sword
 * and sword feet; the spine, the mirrored body, two columns, the rope border; a band count
 * chosen by a bit (every recipe fixes it).
 */

import type { BitReader, BitSlice } from '@/lib/talisman/bits';
import type { BrushSpec, FigureStroke } from '@/lib/talisman/figures';
import type { Pt } from '@/lib/talisman/brush';

export type HeadName = 'dots3' | 'bars3';
export type FootName = 'doubleKnot' | 'knotSword';

export interface TalismanLayout {
  /** The field in lattice units. */
  readonly W: number;
  readonly H: number;
  readonly head: HeadName;
  readonly foot: FootName;
  /** The middle compartment is the core. */
  readonly core: boolean;
  /** How many compartments. */
  readonly bands: number;
  /** The relative heights compartment i chooses from: bandH[i] or bandH[i + 1] (wrapping). */
  readonly bandH: readonly number[];
}

/** How far below the body the foot reaches, both feet: the knots and the sword. */
export const FOOT_DEPTH = 3.4;
/** The space between two compartments: 0.45 below the first, then the divider, 0.35 more. */
export const BELOW = 0.45;
export const BETWEEN = 0.35;
/** BELOW + BETWEEN, the gap the body's scale reserves per divider (the prototype's literal). */
export const GAP = 0.8;
/** The shortest compartment a layout may lay. */
export const MIN_COMPARTMENT = 1.2;

const bar = (y: number, x0: number, x1: number): FigureStroke => ({
  pts: [
    [x0, y],
    [x1, y],
  ],
  horiz: true,
});

const DOT: BrushSpec = { curve: 'corner', entry: 'round', press0: 0.35, exit: 'blunt' };

/** The head's strokes in a field W wide, and where the body starts under it. */
export function headOf(head: HeadName, W: number): { strokes: FigureStroke[]; depth: number } {
  const cx = W / 2;
  switch (head) {
    case 'bars3':
      return {
        strokes: [bar(0.4, 0.4, W - 0.4), bar(1.35, 1.2, W - 1.2), bar(2.3, 0.4, W - 0.4)],
        depth: 3.2,
      };
    case 'dots3': {
      const dots: [number, number][] = [
        [0, 0.5],
        [-1.1, 1.6],
        [1.1, 1.6],
      ];
      return {
        strokes: dots.map(([dx, dy]) => ({
          pts: [
            [cx + dx - 0.22, dy],
            [cx + dx + 0.22, dy + 0.12],
          ],
          brush: DOT,
          weight: 1.3,
        })),
        depth: 2.6,
      };
    }
  }
}

const KNOT: BrushSpec = { curve: 'spline', tension: 6, entry: 'round', press0: 0.1, tail: 1.6 };
const SWORD: BrushSpec = {
  curve: 'corner',
  r: 0.15,
  over: 0,
  entry: 'round',
  press0: 0.3,
  tail: 1.1,
  exit: 'taper',
};

function knot(kx: number, ky: number, R: number, turns: number, dir: number): FigureStroke {
  const pts: Pt[] = [];
  for (let i = 0; i <= 30; i++) {
    const tt = i / 30,
      a = dir * tt * turns * 2 * Math.PI,
      rad = R * (1 - 0.9 * tt);
    pts.push([kx + rad * Math.cos(a), ky + rad * Math.sin(a)]);
  }
  return { pts, brush: KNOT };
}

function sword(x0: number, y0: number, y1: number, hook: boolean): FigureStroke {
  if (!(y0 < y1))
    throw new Error(
      `talisman: no room for the sword (from ${y0} to ${y1}): the stone is too short`
    );
  return {
    pts: hook
      ? [
          [x0, y0],
          [x0, y1],
          [x0 - 0.5, y1 - 0.45],
        ]
      : [
          [x0, y0],
          [x0, y1],
        ],
    brush: SWORD,
  };
}

/**
 * The foot under a body that ends at `y`, centred on `cx`, the sword reaching the field's foot
 * (`H` − 0.3). `bit` is the foot's one bit: the knot's turn (knotSword) or the sword's hook
 * (doubleKnot).
 */
export function footOf(
  foot: FootName,
  cx: number,
  y: number,
  H: number,
  bit: number
): FigureStroke[] {
  switch (foot) {
    case 'knotSword':
      return [knot(cx, y + 1.05, 1.05, 2.2, bit ? 1 : -1), sword(cx, y + 2.3, H - 0.3, true)];
    case 'doubleKnot':
      return [
        knot(cx - 1.2, y + 0.9, 0.85, 2, 1),
        knot(cx + 1.2, y + 0.9, 0.85, 2, -1),
        sword(cx, y + 1.9, H - 0.3, bit === 1),
      ];
  }
}

/** A compartment's box in the field. */
export interface Box {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
  readonly core: boolean;
}

export interface LaidTalisman {
  readonly W: number;
  readonly H: number;
  readonly head: FigureStroke[];
  /** The bits that set the compartments' heights, one per compartment. */
  readonly heights: BitSlice;
  readonly boxes: Box[];
  /** One divider under every compartment but the last. */
  readonly dividers: FigureStroke[];
  /** The foot's bit and strokes. */
  readonly footBit: BitSlice;
  readonly foot: FigureStroke[];
}

/**
 * Lay the talisman, reading its bits in the prototype's order: one per compartment for its
 * height, then one for the foot. The compartments' figures are chosen after (`plan`).
 */
export function layTalisman(t: TalismanLayout, read: BitReader): LaidTalisman {
  const { W, H } = t;
  const cx = W / 2;
  const head = headOf(t.head, W);
  let y = head.depth;
  const nb = t.bands;
  if (!Number.isInteger(nb) || nb < 1) throw new Error(`talisman: ${nb} compartments`);
  const avail = H - y - FOOT_DEPTH;
  const heights = read.read(nb);
  const hs = heights.bits.map((bit, i) => t.bandH[(i + bit) % t.bandH.length]);
  const scale = (avail - (nb - 1) * GAP) / hs.reduce((a, b) => a + b, 0);
  const boxes: Box[] = [];
  const dividers: FigureStroke[] = [];
  for (let i = 0; i < nb; i++) {
    const h = hs[i] * scale;
    // The prototype floors a compartment at 1.2 units, which pushes the foot out of the field;
    // no recipe comes near it (the shortest is 1.3), so a layout that would is refused.
    if (!(h >= MIN_COMPARTMENT))
      throw new Error(
        `talisman: compartment ${i} would be ${h.toFixed(2)} units tall (< ${MIN_COMPARTMENT}) in a ${W} × ${H} field`
      );
    boxes.push({ x: 0.5, y, w: W - 1, h, core: t.core && i === Math.floor(nb / 2) });
    y += h + BELOW;
    if (i < nb - 1) {
      dividers.push(bar(y - 0.1, 0.9, W - 0.9));
      y += BETWEEN;
    }
  }
  const footBit = read.read(1);
  const foot = footOf(t.foot, cx, y, H, footBit.bits[0]);
  return { W, H, head: head.strokes, heights, boxes, dividers, footBit, foot };
}
