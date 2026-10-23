import type { PipelineProposalState } from '@/lib/api';

/**
 * Visual encodings of the transaction pipeline (ported from the approved prototype,
 * docs/superpowers/specs/2026-10-23-transaction-pipeline-prototype.html):
 * - square area = bytes (n×n cells, n ∈ 1…4);
 * - lightness = cycles, four steps of the stage hue;
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

/** Share of the stage hue mixed over the surface at each cycles step. */
export const STEP_I = [0.42, 0.62, 0.82, 1.0] as const;

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
  | 'drop';

export const tok = (name: PipelineToken) => `var(--pipeline-${name})`;

const pct = (t: number) => `${Math.round(t * 1000) / 10}%`;

/** `a` moved toward `b` by `t` (0 = a, 1 = b), as a CSS colour expression. */
export function mixColor(a: string, b: string, t: number): string {
  if (t <= 0) return a;
  if (t >= 1) return b;
  return `color-mix(in srgb, ${b} ${pct(t)}, ${a})`;
}

export const STAGE_BASE: Record<SquareStage, string> = {
  pending: tok('pending'),
  proposed: tok('proposed'),
  committed: tok('committed'),
  cellbase: mixColor(tok('committed'), 'white', 0.25),
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
      fill: mixColor(tok('surface'), base, STEP_I[step]),
      fillOpacity: null,
      stroke: null,
      strokeDasharray: null,
    };
  }
  return {
    fill: base,
    fillOpacity: style === 'hollow' ? 0.14 : 0.06,
    stroke: mixColor(base, 'white', 0.25),
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

/** Proposal chip = the fate of one proposal short id. */
export const CHIP_PAINT: Record<PipelineProposalState, ChipPaint> = {
  early: { fill: tok('proposed'), fillOpacity: 0.12, stroke: tok('gold') },
  waiting: { fill: mixColor(tok('proposed'), 'white', 0.12), fillOpacity: 1, stroke: null },
  committed: {
    fill: mixColor(tok('surface'), tok('committed'), 0.5),
    fillOpacity: 1,
    stroke: null,
  },
  expired: { fill: mixColor(tok('surface'), tok('ink-dim'), 0.5), fillOpacity: 1, stroke: null },
};

export function chipPaint(state: PipelineProposalState): ChipPaint {
  const paint = CHIP_PAINT[state];
  if (!paint) throw new Error(`pipeline: unknown proposal state ${String(state)}`);
  return paint;
}

/** Predicted (NEXT) marks: dashed jade outlines, never filled. */
export const GHOST = {
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

/** The cellbase diamond inside an s×s cell at (x, y). */
export function diamondPath(x: number, y: number, s: number): string {
  const c = s / 2;
  const r = s / 2 + 0.6;
  const f = (v: number) => v.toFixed(1);
  return `M${f(x + c)} ${f(y + c - r)}L${f(x + c + r)} ${f(y + c)}L${f(x + c)} ${f(y + c + r)}L${f(x + c - r)} ${f(y + c)}Z`;
}
