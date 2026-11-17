'use client';

import type { ReactNode } from 'react';
import { FOOTER_ROW } from '@/components/pipeline/svg-text';
import {
  chipPaint,
  diamondPath,
  paintSquare,
  tok,
  type SquarePaint,
} from '@/lib/pipeline/encoding';

/** A key's square: 9 px, the canvas's own shape, in the canvas's own paint. */
const SIDE = 9;

function Sq({
  paint,
}: {
  paint: Pick<SquarePaint, 'fill' | 'fillOpacity' | 'stroke'> & {
    strokeDasharray?: string | null;
  };
}) {
  return (
    <svg width={SIDE + 2} height={SIDE + 2} viewBox={`-1 -1 ${SIDE + 2} ${SIDE + 2}`} aria-hidden>
      <rect
        width={SIDE}
        height={SIDE}
        fillOpacity={paint.fillOpacity ?? undefined}
        strokeDasharray={paint.strokeDasharray ?? undefined}
        style={{ fill: paint.fill, stroke: paint.stroke ?? 'none' }}
      />
    </svg>
  );
}

/** A key: its swatch and its words, never parted when the line wraps. */
function Key({ children, tight = false }: { children: ReactNode; tight?: boolean }) {
  return (
    <span
      className={`inline-flex items-center whitespace-nowrap ${tight ? 'gap-[3px]' : 'gap-1.5'}`}
    >
      {children}
    </span>
  );
}

const solid = (fill: string) => ({ fill, fillOpacity: null, stroke: null });

/**
 * One plain dim line under the canvas (Plate.dc.html `.lg`): the tx square with its scale (the
 * unit is said here only, as on /blocks), the proposal dot, the stage colours as squares in the
 * canvas's own paints (the expired one is a dot's colour), then the cellbase, the cycles shades
 * and the dashed "cycles unknown" square. Everything labelled on the canvas itself (the pool
 * wells, uncles) or explained in a hover or in the "?" title (how to use it) is left out.
 */
export function PipelineLegend() {
  const cellbase = paintSquare('cellbase', 'solid', 0);
  return (
    <div
      data-testid="pipeline-legend"
      className={`${FOOTER_ROW} text-text-dim text-meta gap-x-3.5`}
    >
      <Key>
        <Sq paint={solid(tok('ink-dim'))} />
        tx, area ∝ bytes
      </Key>
      <Key>
        <svg width={8} height={8} viewBox="0 0 8 8" aria-hidden>
          <circle cx={4} cy={4} r={3} style={{ fill: tok('ink-dim') }} />
        </svg>
        proposal
      </Key>
      <span className="bg-base-border h-2.5 w-px" aria-hidden />
      <Key>
        <Sq paint={paintSquare('pending', 'solid', 0)} />
        pending
      </Key>
      <Key>
        <Sq paint={paintSquare('proposed', 'solid', 0)} />
        proposed
      </Key>
      <Key>
        <Sq paint={paintSquare('proposed', 'hollow', 0)} />
        too early
      </Key>
      <Key>
        <Sq paint={paintSquare('committed', 'solid', 0)} />
        committed
      </Key>
      <Key>
        <Sq paint={solid(chipPaint('expired').fill)} />
        expired
      </Key>
      <span className="bg-base-border h-2.5 w-px" aria-hidden />
      <Key>
        <svg
          width={SIDE + 2}
          height={SIDE + 2}
          viewBox={`-1 -1 ${SIDE + 2} ${SIDE + 2}`}
          aria-hidden
        >
          <path d={diamondPath(0, 0, SIDE)} style={{ fill: cellbase.fill }} />
        </svg>
        cellbase
      </Key>
      <Key tight>
        {([0, 1, 2, 3] as const).map((s) => (
          <Sq key={s} paint={paintSquare('committed', 'solid', s)} />
        ))}
        <span className="ml-1">cycles</span>
      </Key>
      <Key>
        <Sq paint={paintSquare('committed', 'dashed', 0)} />
        unknown
      </Key>
    </div>
  );
}
