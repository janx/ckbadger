'use client';

import type { ReactNode } from 'react';
import type { PipelineProposalState } from '@/lib/api';
import {
  chipPaint,
  diamondPath,
  paintSquare,
  tok,
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

/** What each mark on the canvas means. Explanations live here and in the hover, not on the canvas. */
export function PipelineLegend() {
  const cellbase = paintSquare('cellbase', 'solid', 0);
  const committedFill = paintSquare('committed', 'solid', 1).fill;
  return (
    <div
      data-testid="pipeline-legend"
      className="text-text-dim flex flex-wrap gap-x-4 gap-y-1.5 font-mono text-[10px] leading-snug"
    >
      <Key>
        <Sq stage="pending" />
        pending tx
      </Key>
      <Key>
        <Sq stage="proposed" />
        proposed tx
      </Key>
      <Key>
        <Sq stage="proposed" style="hollow" />
        too early to commit
      </Key>
      <Key>
        <Sq stage="committed" />
        committed tx
      </Key>
      <Key>
        <svg width={10} height={10} viewBox="0 0 10 10" aria-hidden>
          <path d={diamondPath(1.5, 1.5, 7)} style={{ fill: cellbase.fill }} />
        </svg>
        cellbase
      </Key>
      <Key tight>
        <Chip state="waiting" />
        <Chip state="early" />
        <Chip state="committed" />
        <Chip state="expired" />
        <span className="ml-1">proposals: waiting · too early · committed later · expired</span>
      </Key>
      <Key>
        <svg width={34} height={12} viewBox="0 0 34 12" aria-hidden>
          <path d="M2 1V11M32 1V11" style={{ stroke: tok('gold') }} />
          <path d="M2 6H32" strokeWidth={1.5} style={{ stroke: tok('gold') }} />
        </svg>
        proposal window
      </Key>
      <Key>
        <svg width={22} height={12} viewBox="0 0 22 12" aria-hidden>
          <rect
            x={0.5}
            y={0.5}
            width={21}
            height={11}
            rx={2.5}
            strokeDasharray="2 2"
            style={{ fill: tok('surface'), stroke: tok('ink-dim') }}
          />
          <rect
            x={4}
            y={5}
            width={5}
            height={5}
            rx={1}
            style={{ fill: chipPaint('waiting').fill }}
          />
          <rect
            x={11}
            y={5}
            width={5}
            height={5}
            rx={1}
            style={{ fill: chipPaint('committed').fill }}
          />
        </svg>
        uncle, carried inside the block that includes it
      </Key>
      <Key>
        <svg width={26} height={8} viewBox="0 -4 26 8" aria-hidden>
          <path
            d="M1 0L24 0M20 -3L24 0L20 3"
            fill="none"
            strokeOpacity={0.85}
            style={{ stroke: tok('jade') }}
          />
        </svg>
        parent link
      </Key>
      <Key tight>
        {([0, 1, 2, 3] as const).map((s) => (
          <Sq key={s} stage="committed" step={s} />
        ))}
        <span className="ml-1">cycles: fewer → more</span>
      </Key>
      <Key>
        <Sq stage="committed" style="dashed" />
        cycles unknown
      </Key>
      <Key>
        <svg width={24} height={14} viewBox="0 0 24 14" aria-hidden>
          <rect x={0} y={7} width={6} height={6} rx={1} style={{ fill: committedFill }} />
          <rect x={9} y={0} width={13} height={13} rx={1.5} style={{ fill: committedFill }} />
        </svg>
        area = bytes
      </Key>
    </div>
  );
}
