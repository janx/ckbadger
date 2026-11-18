'use client';

import { useId, type ReactNode } from 'react';
import { color } from '@/lib/palette';
import { LINK_PAINT, WELL_FILL } from '@/lib/pipeline/encoding';
import {
  G,
  flowPaths,
  wellRect,
  wellShade,
  type Scene,
  type ScenePoolGrid,
} from '@/lib/pipeline/layout';
import type { DimLevel } from '@/components/pipeline/block-card';
import { T10, T10B, T11C, fmtInt } from '@/components/pipeline/svg-text';
import { TxSquare } from '@/components/pipeline/tx-square';
import { formatBlockNumber } from '@/lib/format-block';

interface PoolZoneProps {
  scene: Scene;
  /** Per-tx dim level (window focus); absent = lit. */
  dimOf?: (hash: string) => DimLevel;
  onIds?: ReadonlySet<string>;
}

function Grid({
  grid,
  dimOf,
  onIds,
  testId,
}: { grid: ScenePoolGrid; testId: string } & Omit<PoolZoneProps, 'scene'>) {
  return (
    <g data-pool-grid={testId}>
      {grid.squares.map((sq) => (
        <g key={sq.id} data-dim={dimOf ? dimOf(sq.id) : 'false'}>
          {/* loose in the well: a well does not age and cuts nothing */}
          <TxSquare sq={sq} age={null} on={onIds?.has(sq.id)} />
        </g>
      ))}
    </g>
  );
}

/**
 * A pool: a well, never a block. Square, recessed below the page under a 1 px edge in the line
 * ink, the theme's inset a 3 px shade under that edge, fading to nothing (Plate.dc.html `.wl`).
 * Its name in the stage's colour and its pool total stand on the head line above it, where a
 * block has its number; in it, what waits lies loose, drawn at the scale it has in a block.
 */
function PoolWell({
  x,
  title,
  tone,
  count,
  testId,
  children,
}: {
  x: number;
  title: string;
  tone: 'aqua' | 'gold';
  count: string;
  testId: string;
  children?: ReactNode;
}) {
  const shade = `pipeline-inset${useId().replace(/[^\w-]/g, '')}`;
  const fill = tone === 'aqua' ? 'fill-aqua' : 'fill-gold';
  return (
    <g data-pool-card={testId}>
      <rect
        data-tray
        {...wellRect(x)}
        strokeWidth={1}
        vectorEffect="non-scaling-stroke"
        style={{ fill: WELL_FILL, stroke: color('base-border') }}
      />
      <defs>
        <linearGradient id={shade} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopOpacity={1} style={{ stopColor: 'var(--m-well-inset)' }} />
          <stop offset="1" stopOpacity={0} style={{ stopColor: 'var(--m-well-inset)' }} />
        </linearGradient>
      </defs>
      <rect data-inset {...wellShade(x)} fill={`url(#${shade})`} pointerEvents="none" />
      <text x={x + G.poolHeadPad} y={G.headY + 3.5} className={T11C} pointerEvents="none">
        <tspan className={fill}>{title}</tspan>
      </text>
      <text
        data-count
        x={x + G.cardW - G.poolHeadPad}
        y={G.headY + 3.5}
        textAnchor="end"
        className={T10}
        pointerEvents="none"
      >
        {count}
      </text>
      {children}
    </g>
  );
}

/**
 * Arrowheads through a gap, in the muted ink (direction G): the tx moves on to the next stage. The
 * row's (`flowPaths`) and the column's (`columnFlowPaths`).
 */
export function FlowLink({ d }: { d: string }) {
  return (
    <path
      data-flow
      d={d}
      fillOpacity={LINK_PAINT.flow.fillOpacity}
      pointerEvents="none"
      style={{ fill: LINK_PAINT.flow.fill }}
    />
  );
}

/**
 * The tx lifetime starts here, before NEXT: the mempool well (pending: no block has proposed them)
 * and the proposed well (a block in the window proposed them; a block commits them next). Both
 * stages come from the chain's proposal zones.
 */
export function PoolZone({ scene, dimOf, onIds }: PoolZoneProps) {
  const { snapshot, pool } = scene;
  const flows = flowPaths();
  return (
    <g data-pool>
      <PoolWell
        x={G.mempoolX}
        title="mempool"
        tone="aqua"
        count={pool ? fmtInt(pool.pending.total) : '—'}
        testId="pending"
      >
        {pool ? (
          <Grid grid={pool.pending} dimOf={dimOf} onIds={onIds} testId="pending" />
        ) : (
          <text data-pool-status x={G.mempoolX + G.padX} y={G.poolTop + 10} className={T10B}>
            <title>
              {`The node's tx pool is at ${formatBlockNumber(snapshot.poolTip.number)} (${snapshot.poolTip.hash}) but ckbadger has synced ${formatBlockNumber(snapshot.tip.number)} (${snapshot.tip.hash}). The pool view returns once both are at the same block.`}
            </title>
            unavailable
          </text>
        )}
      </PoolWell>
      <FlowLink d={flows.toProposed} />
      <PoolWell
        x={G.proposedX}
        title="proposed"
        tone="gold"
        count={pool ? fmtInt(pool.proposed.total) : '—'}
        testId="proposed"
      >
        {pool && <Grid grid={pool.proposed} dimOf={dimOf} onIds={onIds} testId="proposed" />}
      </PoolWell>
      <FlowLink d={flows.toNext} />
    </g>
  );
}
