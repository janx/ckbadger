'use client';

import type { ReactNode } from 'react';
import type { PipelineProposalState } from '@/lib/api';
import {
  chipPaint,
  diamondPath,
  paintSquare,
  type ShadeStep,
  type SquareStage,
  type SquareStyle,
} from '@/lib/pipeline/encoding';

function Sq({
  stage,
  step = 1,
  style = 'solid',
  size = 8,
}: {
  stage: SquareStage;
  step?: ShadeStep;
  style?: SquareStyle;
  size?: number;
}) {
  const p = paintSquare(stage, style, step);
  return (
    <svg width={size + 2} height={size + 2} viewBox={`-1 -1 ${size + 2} ${size + 2}`} aria-hidden>
      <rect
        width={size}
        height={size}
        rx={1.5}
        fillOpacity={p.fillOpacity ?? undefined}
        strokeDasharray={p.strokeDasharray ?? undefined}
        style={{ fill: p.fill, stroke: p.stroke ?? 'none' }}
      />
    </svg>
  );
}

function Chip({ state }: { state: PipelineProposalState }) {
  const p = chipPaint(state);
  return (
    <svg width={7} height={7} viewBox="-1 -1 7 7" aria-hidden>
      <rect
        width={5}
        height={5}
        rx={1}
        fillOpacity={p.fillOpacity}
        style={{ fill: p.fill, stroke: p.stroke ?? 'none' }}
      />
    </svg>
  );
}

function Key({ children, tight = false }: { children: ReactNode; tight?: boolean }) {
  return (
    <span
      className={`inline-flex items-center whitespace-nowrap ${tight ? 'gap-[3px]' : 'gap-1.5'}`}
    >
      {children}
    </span>
  );
}

/**
 * One quiet line: the stage colours (tx squares and proposal chips share them), the cellbase, the
 * cycles shades and the dashed outline. Everything labelled on the canvas itself (the proposal
 * window, uncles), explained in a hover or in the "?" title (area = bytes, how to use it) is left
 * out.
 */
export function PipelineLegend() {
  const cellbase = paintSquare('cellbase', 'solid', 0);
  return (
    <div
      data-testid="pipeline-legend"
      className="text-text-dim flex flex-wrap items-center gap-x-3.5 gap-y-1 font-mono text-[10px] leading-snug"
    >
      <Key>
        <Sq stage="pending" />
        pending
      </Key>
      <Key>
        <Sq stage="proposed" />
        proposed
      </Key>
      <Key>
        <Sq stage="proposed" style="hollow" />
        too early
      </Key>
      <Key>
        <Sq stage="committed" />
        committed
      </Key>
      <Key>
        <Chip state="expired" />
        expired
      </Key>
      <span className="bg-base-border h-2.5 w-px" aria-hidden />
      <Key>
        <svg width={10} height={10} viewBox="0 0 10 10" aria-hidden>
          <path d={diamondPath(1.5, 1.5, 7)} style={{ fill: cellbase.fill }} />
        </svg>
        cellbase
      </Key>
      <Key tight>
        {([0, 1, 2, 3] as const).map((s) => (
          <Sq key={s} stage="committed" step={s} size={6} />
        ))}
        <span className="ml-1">cycles</span>
      </Key>
      <Key>
        <Sq stage="committed" style="dashed" />
        unknown
      </Key>
    </div>
  );
}
