'use client';

import type { ReactNode } from 'react';
import { FOOTER_ROW } from '@/components/pipeline/svg-text';
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

/** A colour key, shape-neutral: a stage paints both tx squares and proposal dots. */
function Swatch({ fill, stroke = null }: { fill: string; stroke?: string | null }) {
  return (
    <svg width={12} height={6} viewBox="-1 -1 12 6" aria-hidden>
      <rect
        width={10}
        height={4}
        rx={2}
        fillOpacity={stroke ? 0.14 : 1}
        style={{ fill, stroke: stroke ?? 'none' }}
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
 * One quiet line: the two shapes (tx square, proposal dot), the stage colours they share (as
 * shape-neutral swatches), the cellbase, the cycles shades and the dashed "cycles unknown" square.
 * Everything labelled on the canvas itself (the pool wells, uncles), explained in a hover or in the
 * "?" title (area = bytes, how to use it) is left out.
 */
export function PipelineLegend() {
  const cellbase = paintSquare('cellbase', 'solid', 0);
  return (
    <div
      data-testid="pipeline-legend"
      className={`${FOOTER_ROW} text-text-dim gap-x-3.5 text-[10px]`}
    >
      <Key>
        <svg width={9} height={9} viewBox="0 0 9 9" aria-hidden>
          <rect x={0.5} y={0.5} width={8} height={8} rx={1.5} style={{ fill: tok('ink') }} />
        </svg>
        tx
      </Key>
      <Key>
        <svg width={8} height={8} viewBox="0 0 8 8" aria-hidden>
          <circle cx={4} cy={4} r={3} style={{ fill: tok('ink') }} />
        </svg>
        proposal
      </Key>
      <span className="bg-base-border h-2.5 w-px" aria-hidden />
      <Key>
        <Swatch fill={paintSquare('pending', 'solid', 0).fill} />
        pending
      </Key>
      <Key>
        <Swatch fill={paintSquare('proposed', 'solid', 0).fill} />
        proposed
      </Key>
      <Key>
        <Swatch fill={tok('proposed')} stroke={tok('gold')} />
        too early
      </Key>
      <Key>
        <Swatch fill={paintSquare('committed', 'solid', 0).fill} />
        committed
      </Key>
      <Key>
        <Swatch fill={chipPaint('expired').fill} />
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
