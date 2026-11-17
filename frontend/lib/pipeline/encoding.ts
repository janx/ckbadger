import type { PipelineProposalState } from '@/lib/api';
import { color } from '@/lib/palette';

/**
 * Visual encodings of the transaction pipeline (ported from the approved prototype,
 * docs/superpowers/specs/2026-10-23-transaction-pipeline-prototype.html):
 * - a tx is a square, area = bytes (n×n cells, n ∈ 1…4); a proposal short id is a dot;
 * - cycles: four steps of the stage hue mixed toward the theme's step-to ink (lighter by night,
 *   inkier by day, so every step reads on the cut in both themes);
 * - hollow = proposed but too early to commit; dashed = cycles unknown; predicted = dashed.
 *
 * Colours are CSS custom properties (`--pipeline-*`, app/globals.css) mixed with `color-mix`, so
 * every painted value stays a token expression.
 */

export type SquareSide = 1 | 2 | 3 | 4;
export type ShadeStep = 0 | 1 | 2 | 3;
export type SquareStage = 'pending' | 'proposed' | 'committed' | 'cellbase';
/** solid · hollow (too early to commit) · dashed (cycles unknown). */
export type SquareStyle = 'solid' | 'hollow' | 'dashed';

const clamp = (lo: number, hi: number, v: number) => Math.min(hi, Math.max(lo, v));
const clamp01 = (v: number) => clamp(0, 1, v);

/** Square side in grid cells: area ∝ serialized bytes. */
export function squareSide(size: number): SquareSide {
  if (!(size > 0)) throw new Error(`pipeline: tx size must be a positive byte count, got ${size}`);
  return clamp(1, 4, Math.round(Math.sqrt(size / 600))) as SquareSide;
}

/** Cycles on a log scale, 0 at ≤ 1 M cycles and 1 at ≥ ~158 M. */
export function cyclesGlow(cycles: number): number {
  return clamp01((Math.log10(Math.max(1, cycles)) - 6.0) / 2.2);
}

export function shadeStep(cycles: number): ShadeStep {
  const g = cyclesGlow(cycles);
  return g < 0.15 ? 0 : g < 0.4 ? 1 : g < 0.7 ? 2 : 3;
}

/**
 * Share of the step-to ink mixed into the stage hue at each cycles step: more cycles, further from
 * the stage hue (lighter by night, inkier by day).
 */
export const STEP_W = [0, 0.18, 0.36, 0.54] as const;

export interface TxLookInput {
  stage: SquareStage;
  /** null = not computed (yet); only committed txs can lack cycles. */
  cycles: number | null;
  /** Proposed in the latest block, so the next block cannot commit it yet. */
  tooEarly: boolean;
}

export interface TxLook {
  stage: SquareStage;
  style: SquareStyle;
  step: ShadeStep;
}

export function txLook({ stage, cycles, tooEarly }: TxLookInput): TxLook {
  if (tooEarly && stage !== 'proposed') {
    throw new Error(`pipeline: only a proposed tx can be too early to commit, got ${stage}`);
  }
  if (stage === 'cellbase') return { stage, style: 'solid', step: 0 };
  if (cycles === null) {
    if (stage !== 'committed') throw new Error(`pipeline: a ${stage} pool tx has no cycles`);
    return { stage, style: 'dashed', step: 0 };
  }
  return { stage, style: tooEarly ? 'hollow' : 'solid', step: shadeStep(cycles) };
}

export type PipelineToken =
  | 'surface'
  | 'elevated'
  | 'border'
  | 'border-subtle'
  | 'ink-bright'
  | 'ink'
  | 'ink-dim'
  | 'jade'
  | 'aqua'
  | 'gold'
  | 'pending'
  | 'proposed'
  | 'committed'
  | 'ember'
  | 'drop'
  | 'well'
  | 'step-to'
  | 'plate'
  | 'cut'
  | 'dot-waiting'
  | 'dot-committed'
  | 'dot-expired';

export const tok = (name: PipelineToken) => `var(--pipeline-${name})`;

const pct = (t: number) => `${Math.round(t * 1000) / 10}%`;

/** `a` moved toward `b` by `t` (0 = a, 1 = b), as a CSS colour expression. */
export function mixColor(a: string, b: string, t: number): string {
  if (t <= 0) return a;
  if (t >= 1) return b;
  return `color-mix(in srgb, ${b} ${pct(t)}, ${a})`;
}

/**
 * The materials of the canvas: a pool is a well (recessed below the surface), a mined block a solid
 * plate of its own tone (no outline at rest, never faded), every transaction on it set in a cut of
 * a second tone (2 px round each square and the cellbase, and only there), and NEXT the only dashed
 * outline (`GHOST`, predicted).
 */
export const WELL_FILL = tok('well');
export const PLATE_FILL = tok('plate');
export const CUT_FILL = tok('cut');

/** How many blocks behind the tip the age fade bottoms out. */
const AGE_STEPS = 7;

/** Opacity of a block `age` places behind the tip: 1 at the tip, 0.4 from tip−7 on. */
export function ageOpacity(age: number): number {
  if (!Number.isInteger(age) || age < 0) {
    throw new Error(`pipeline: a block age is a count of blocks behind the tip, got ${age}`);
  }
  return 1 - (0.6 * Math.min(age, AGE_STEPS)) / AGE_STEPS;
}

/** A mark that steps back with its block's age: a square, the cellbase diamond, a proposal dot. */
export type AgedMark = 'square' | 'cellbase' | 'dot';

/** What an aged mark is mixed toward: the ground it is drawn on (a square sits in its cut). */
const AGE_GROUND: Record<AgedMark, string> = {
  square: CUT_FILL,
  cellbase: CUT_FILL,
  dot: PLATE_FILL,
};

/**
 * The fill of a mark on a block `age` places behind the tip: its own colour mixed toward its
 * ground, keeping `ageOpacity(age)` of itself. A mark is mixed, never made transparent (opacity
 * would show the plate through a square and its cut); the plate itself and the words never fade.
 */
export function agedFill(mark: AgedMark, fill: string, age: number): string {
  const ground = AGE_GROUND[mark];
  if (!ground) throw new Error(`pipeline: an aged ${String(mark)} has no ground to fade toward`);
  return mixColor(ground, fill, ageOpacity(age));
}

export const STAGE_BASE: Record<SquareStage, string> = {
  pending: tok('pending'),
  proposed: tok('proposed'),
  committed: tok('committed'),
  cellbase: mixColor(tok('committed'), tok('step-to'), 0.25),
};

export interface SquarePaint {
  fill: string;
  fillOpacity: number | null;
  stroke: string | null;
  strokeDasharray: string | null;
}

/** Paint of a square from its base colour (a stage base, or a tween between two). */
export function paintFromBase(base: string, style: SquareStyle, step: ShadeStep): SquarePaint {
  if (style === 'solid') {
    return {
      fill: mixColor(base, tok('step-to'), STEP_W[step]),
      fillOpacity: null,
      stroke: null,
      strokeDasharray: null,
    };
  }
  return {
    fill: base,
    fillOpacity: style === 'hollow' ? 0.14 : 0.06,
    stroke: mixColor(base, tok('step-to'), 0.25),
    strokeDasharray: style === 'dashed' ? '2 1.5' : null,
  };
}

export function paintSquare(stage: SquareStage, style: SquareStyle, step: ShadeStep): SquarePaint {
  if (stage === 'cellbase') {
    return { fill: STAGE_BASE.cellbase, fillOpacity: null, stroke: null, strokeDasharray: null };
  }
  return paintFromBase(STAGE_BASE[stage], style, step);
}

export interface ChipPaint {
  fill: string;
  fillOpacity: number;
  stroke: string | null;
}

/**
 * Proposal dot = the fate of one proposal short id, in the stage colours the tx squares use. The
 * solid dots are tokens of their own, derived against the plate they stand on (≥ 3 : 1 for
 * waiting and committed in both themes; the expired dot is the quiet one).
 */
export const CHIP_PAINT: Record<PipelineProposalState, ChipPaint> = {
  early: { fill: tok('proposed'), fillOpacity: 0.12, stroke: tok('gold') },
  waiting: { fill: tok('dot-waiting'), fillOpacity: 1, stroke: null },
  committed: { fill: tok('dot-committed'), fillOpacity: 1, stroke: null },
  expired: { fill: tok('dot-expired'), fillOpacity: 1, stroke: null },
};

export function chipPaint(state: PipelineProposalState): ChipPaint {
  const paint = CHIP_PAINT[state];
  if (!paint) throw new Error(`pipeline: unknown proposal state ${String(state)}`);
  return paint;
}

/**
 * The paint of a square or the cellbase on a block `age` places behind the tip: its fill and its
 * outline (a hollow or a dashed square's) mixed toward the cut it sits in (`agedFill`); `null` for
 * a square loose in a pool well, which does not age. The static render and the motion layer both
 * paint a mark through this.
 */
export function agedPaint(
  p: SquarePaint,
  mark: Exclude<AgedMark, 'dot'>,
  age: number | null
): SquarePaint {
  if (age === null) return p;
  return {
    ...p,
    fill: agedFill(mark, p.fill, age),
    stroke: p.stroke === null ? null : agedFill(mark, p.stroke, age),
  };
}

/** The paint of a proposal dot on a block `age` places behind the tip: mixed toward the plate. */
export function agedChip(state: PipelineProposalState, age: number): ChipPaint {
  const p = chipPaint(state);
  return {
    ...p,
    fill: agedFill('dot', p.fill, age),
    stroke: p.stroke === null ? null : agedFill('dot', p.stroke, age),
  };
}

/**
 * A hovered plate's edge: 1 px of the dim ink (4.1 : 1 on the plate, a line, not a word). The plate
 * has no stroke at rest.
 */
export const PLATE_EDGE = color('text-dim');

/**
 * Predicted (NEXT) marks: dashed jade squares and hollow gold dots, never solid; NEXT itself (and a
 * landing block until its commits land) a dashed outline with no fill, the only one.
 */
export const GHOST = {
  outline: { stroke: tok('jade'), strokeOpacity: 0.6, dash: '2 2' },
  square: {
    fill: tok('jade'),
    fillOpacity: 0.05,
    stroke: tok('jade'),
    strokeOpacity: 0.7,
    dash: '2 1.5',
  },
  cellbase: {
    fill: tok('jade'),
    fillOpacity: 0.06,
    stroke: tok('jade'),
    strokeOpacity: 0.7,
    dash: '1.5 1.5',
  },
  chip: { stroke: tok('gold'), strokeOpacity: 0.5 },
} as const;

/**
 * The chain's links, two bands of filled ticks each (never stroked): the muted ink between two
 * blocks, the same at 0.7 from NEXT (and from a block still landing), gold inside the drawn
 * proposal window, at full strength. The pool's flows: one faint band of the muted ink at 0.8.
 */
export const LINK_PAINT = {
  solid: { fill: tok('ink-dim'), fillOpacity: null },
  pend: { fill: tok('ink-dim'), fillOpacity: 0.7 },
  win: { fill: tok('gold'), fillOpacity: null },
  flow: { fill: tok('ink-dim'), fillOpacity: 0.8 },
} as const;

/**
 * The chain's tail past the last plate (its two bands fading to nothing): gold while the drawn
 * window runs on past that plate, else the muted ink of a link.
 */
export const tailFill = (windowRunsOn: boolean) => tok(windowRunsOn ? 'gold' : 'ink-dim');

/** The cellbase diamond inside an s×s cell at (x, y). */
export function diamondPath(x: number, y: number, s: number): string {
  const c = s / 2;
  const r = s / 2 + 0.6;
  const f = (v: number) => v.toFixed(1);
  return `M${f(x + c)} ${f(y + c - r)}L${f(x + c + r)} ${f(y + c)}L${f(x + c)} ${f(y + c + r)}L${f(x + c - r)} ${f(y + c)}Z`;
}
