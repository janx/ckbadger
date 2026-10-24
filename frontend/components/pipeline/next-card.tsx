'use client';

import { GHOST, diamondPath, tok } from '@/lib/pipeline/encoding';
import { G, tail4, type SceneNext } from '@/lib/pipeline/layout';
import type { DimLevel } from '@/components/pipeline/block-card';
import { T10, T11C } from '@/components/pipeline/svg-text';

interface NextColumnProps {
  available: boolean;
}

/** Nothing labels NEXT (its dashed card says "predicted"); only a missing prediction is said. */
export function NextColumn({ available }: NextColumnProps) {
  if (available) return null;
  return (
    <text
      data-next-column
      x={G.nextX + G.padX}
      y={G.cardTop + 34}
      className={T10}
      pointerEvents="none"
    >
      unavailable
    </text>
  );
}

interface NextCardProps {
  next: SceneNext;
  highlighted: boolean;
  dim: DimLevel;
}

/** The predicted next block: dashed, because none of it is real yet. */
export function NextCard({ next, highlighted, dim }: NextCardProps) {
  return (
    <g data-next transform={`translate(${next.x},0)`} className="cursor-pointer">
      <g data-dim={dim}>
        <rect
          data-box
          x={0.5}
          y={G.cardTop + 0.5}
          width={Math.max(1, next.w - 1)}
          height={Math.max(1, next.h - 1)}
          rx={6}
          strokeWidth={1}
          strokeDasharray="3 2"
          vectorEffect="non-scaling-stroke"
          fillOpacity={highlighted ? 0.9 : 0.5}
          strokeOpacity={highlighted ? 1 : 0.6}
          style={{ fill: tok('elevated'), stroke: tok('jade') }}
        />
        <text x={G.padX} y={G.chainY + 3.5} className={T11C}>
          {tail4(next.number)}
        </text>
        <g data-ghost-chips>
          {next.ghostChips.map((k, i) => (
            <circle
              key={i}
              data-ghost-chip
              cx={k.x + G.chip / 2}
              cy={k.y + G.chip / 2}
              r={G.chip / 2 - 0.5}
              fill="none"
              strokeWidth={1}
              strokeOpacity={GHOST.chip.strokeOpacity}
              vectorEffect="non-scaling-stroke"
              style={{ stroke: GHOST.chip.stroke }}
            />
          ))}
          {next.chipOverflow > 0 && (
            <text
              x={next.w - G.padX}
              y={G.sepY - 1}
              textAnchor="end"
              className={T10}
            >{`+${next.chipOverflow}`}</text>
          )}
        </g>
        <g data-ghosts>
          {next.ghosts.map((q, i) =>
            q.cellbase ? (
              <path
                key={i}
                data-ghost="cellbase"
                d={diamondPath(q.x, q.y, q.s)}
                fillOpacity={GHOST.cellbase.fillOpacity}
                strokeOpacity={GHOST.cellbase.strokeOpacity}
                strokeWidth={1}
                strokeDasharray={GHOST.cellbase.dash}
                vectorEffect="non-scaling-stroke"
                style={{ fill: GHOST.cellbase.fill, stroke: GHOST.cellbase.stroke }}
              />
            ) : (
              <rect
                key={q.id ?? i}
                data-ghost="tx"
                data-ghost-tx={q.id ?? undefined}
                x={q.x + 0.5}
                y={q.y + 0.5}
                width={q.s - 1}
                height={q.s - 1}
                rx={1}
                fillOpacity={GHOST.square.fillOpacity}
                strokeOpacity={GHOST.square.strokeOpacity}
                strokeWidth={1}
                strokeDasharray={GHOST.square.dash}
                vectorEffect="non-scaling-stroke"
                style={{ fill: GHOST.square.fill, stroke: GHOST.square.stroke }}
              />
            )
          )}
        </g>
        {next.hidden > 0 && (
          <text
            data-count
            x={next.w - G.padX}
            y={G.chainY + 3.5}
            textAnchor="end"
            className={T10}
          >{`+${next.hidden}`}</text>
        )}
      </g>
    </g>
  );
}
