'use client';

import type { ReactNode } from 'react';
import { WELL_FILL, tok } from '@/lib/pipeline/encoding';
import { CARD_H, G, type Scene, type ScenePoolGrid } from '@/lib/pipeline/layout';
import type { DimLevel } from '@/components/pipeline/block-card';
import { T10, T10B, T11C, fmtInt } from '@/components/pipeline/svg-text';
import { TxSquare } from '@/components/pipeline/tx-square';

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
          <TxSquare sq={sq} on={onIds?.has(sq.id)} />
        </g>
      ))}
    </g>
  );
}

/**
 * A pool: a labelled well, never a block. It sits below the canvas surface (no stroke) where a
 * block stands above it as a tile. The label sits on the header row like a block's number, in the
 * stage's colour, with the stage's pool total right-aligned; under it the well holds its txs.
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
  const l = x + 0.5;
  const r = x + G.cardW - 0.5;
  const t = G.cardTop + G.headH + 0.5;
  const b = G.cardTop + CARD_H - 0.5;
  const q = 6;
  const walls = `M${l} ${t}V${b - q}Q${l} ${b} ${l + q} ${b}H${r - q}Q${r} ${b} ${r} ${b - q}V${t}`;
  const fill = tone === 'aqua' ? 'fill-aqua' : 'fill-gold';
  return (
    <g data-pool-card={testId}>
      <path data-tray d={`${walls}Z`} stroke="none" style={{ fill: WELL_FILL }} />
      <text x={x + G.padX} y={G.chainY + 3.5} className={T11C} pointerEvents="none">
        <tspan className={fill}>{title}</tspan>
      </text>
      <text
        data-count
        x={x + G.cardW - G.padX}
        y={G.chainY + 3.5}
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

/** The dashed line through the header row (no arrowhead): the tx moves on to the next stage. */
function FlowLink({ x0, x1 }: { x0: number; x1: number }) {
  const y = G.chainY + 0.5;
  return (
    <path
      data-flow
      d={`M${x0 + 2} ${y}L${x1 - 2} ${y}`}
      fill="none"
      strokeWidth={1}
      strokeDasharray="3 3"
      strokeOpacity={0.55}
      vectorEffect="non-scaling-stroke"
      pointerEvents="none"
      style={{ stroke: tok('jade') }}
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
              {`The node's tx pool is at #${fmtInt(snapshot.poolTip.number)} (${snapshot.poolTip.hash}) but ckbadger has synced #${fmtInt(snapshot.tip.number)} (${snapshot.tip.hash}). The pool view returns once both are at the same block.`}
            </title>
            unavailable
          </text>
        )}
      </PoolWell>
      <FlowLink x0={G.mempoolX + G.cardW} x1={G.proposedX} />
      <PoolWell
        x={G.proposedX}
        title="proposed"
        tone="gold"
        count={pool ? fmtInt(pool.proposed.total) : '—'}
        testId="proposed"
      >
        {pool && <Grid grid={pool.proposed} dimOf={dimOf} onIds={onIds} testId="proposed" />}
      </PoolWell>
      <FlowLink x0={G.proposedX + G.cardW} x1={G.nextX} />
    </g>
  );
}
