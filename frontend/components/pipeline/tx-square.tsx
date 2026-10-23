'use client';

import { diamondPath, paintSquare } from '@/lib/pipeline/encoding';
import type { SceneSquare } from '@/lib/pipeline/layout';

interface TxSquareProps {
  sq: SceneSquare;
  /** Hovered or pinned: kept bright while the rest steps back. */
  on?: boolean;
}

/** One transaction: an n×n square (area = bytes); the cellbase is a diamond. */
export function TxSquare({ sq, on = false }: TxSquareProps) {
  const paint = paintSquare(sq.stage, sq.style, sq.step);
  if (sq.kind === 'cellbase') {
    return (
      <path
        data-sq={sq.id}
        data-stage="cellbase"
        className={on ? 'is-on' : undefined}
        d={diamondPath(sq.x, sq.y, sq.s)}
        style={{ fill: paint.fill }}
      />
    );
  }
  return (
    <rect
      data-sq={sq.id}
      data-stage={sq.stage}
      data-style={sq.style}
      data-step={sq.step}
      className={on ? 'is-on' : undefined}
      x={sq.x}
      y={sq.y}
      width={sq.s}
      height={sq.s}
      rx={Math.min(1.5, sq.s * 0.2)}
      fillOpacity={paint.fillOpacity ?? undefined}
      strokeWidth={paint.stroke ? 1 : undefined}
      strokeDasharray={paint.strokeDasharray ?? undefined}
      vectorEffect="non-scaling-stroke"
      style={{ fill: paint.fill, stroke: paint.stroke ?? 'none' }}
    />
  );
}
