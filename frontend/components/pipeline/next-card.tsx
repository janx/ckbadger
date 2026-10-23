'use client';

import { GHOST, diamondPath, tok } from '@/lib/pipeline/encoding';
import { G, tail4, type SceneNext } from '@/lib/pipeline/layout';
import type { DimLevel } from '@/components/pipeline/block-card';
import { T10, T10C, T11C, TZ } from '@/components/pipeline/svg-text';

interface NextColumnProps {
  available: boolean;
}

/** The NEXT zone title, and why its card is missing when the pool is unavailable. */
export function NextColumn({ available }: NextColumnProps) {
  return (
    <g data-next-column pointerEvents="none">
      <text x={G.nextX} y={16} className={TZ}>
        Next
      </text>
      {!available && (
        <text x={G.nextX} y={30} className={T10}>
          unavailable
        </text>
      )}
    </g>
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
          fillOpacity={highlighted ? 0.9 : 0.35}
          strokeOpacity={highlighted ? 1 : 0.3}
          style={{ fill: tok('elevated'), stroke: tok('jade') }}
        />
        <line
          x1={1}
          x2={next.w - 1}
          y1={G.cardTop + G.headH + 0.5}
          y2={G.cardTop + G.headH + 0.5}
          strokeWidth={1}
          vectorEffect="non-scaling-stroke"
          style={{ stroke: tok('border') }}
        />
        <line
          x1={G.padX}
          x2={next.w - G.padX}
          y1={G.sepY + 0.5}
          y2={G.sepY + 0.5}
          strokeWidth={1}
          vectorEffect="non-scaling-stroke"
          style={{ stroke: tok('border') }}
        />
        <text x={G.padX} y={G.chainY + 3.5} className={T11C}>
          {tail4(next.number)}
        </text>
        <g data-ghost-chips>
          {next.ghostChips.map((k, i) => (
            <rect
              key={i}
              data-ghost-chip
              x={k.x}
              y={k.y}
              width={G.chip}
              height={G.chip}
              rx={1}
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
          <text data-hidden x={G.padX} y={G.cardTop + next.h + 11} className={T10C}>
            {`+${next.hidden}`}
          </text>
        )}
        <text
          data-count
          x={next.w - 2}
          y={G.cardTop + next.h + 11}
          textAnchor="end"
          className={T10}
        >{`~${next.predicted + 1} tx`}</text>
      </g>
    </g>
  );
}
