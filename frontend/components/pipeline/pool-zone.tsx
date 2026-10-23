'use client';

import { tok } from '@/lib/pipeline/encoding';
import { G, U, type Scene, type ScenePoolGrid } from '@/lib/pipeline/layout';
import type { DimLevel } from '@/components/pipeline/block-card';
import { T10, T10B, T10C, TZ, fmtInt } from '@/components/pipeline/svg-text';
import { TxSquare } from '@/components/pipeline/tx-square';

interface PoolZoneProps {
  scene: Scene;
  /** Per-tx dim level (window focus); absent = lit. */
  dimOf?: (hash: string) => DimLevel;
  onIds?: ReadonlySet<string>;
}

function Grid({
  grid,
  top,
  dimOf,
  onIds,
  testId,
}: { grid: ScenePoolGrid; top: number; testId: string } & Omit<PoolZoneProps, 'scene'>) {
  return (
    <g data-pool-grid={testId}>
      {grid.squares.map((sq) => (
        <g key={sq.id} data-dim={dimOf ? dimOf(sq.id) : 'false'}>
          <TxSquare sq={sq} on={onIds?.has(sq.id)} />
        </g>
      ))}
      {grid.hidden > 0 && (
        <text
          data-hidden
          x={G.memX0 + G.memCols * U - 1}
          y={top - 8}
          textAnchor="end"
          className={T10C}
        >{`+${grid.hidden}`}</text>
      )}
    </g>
  );
}

/** MEMPOOL: the node's tx pool split by the chain's proposal zones into pending and proposed. */
export function PoolZone({ scene, dimOf, onIds }: PoolZoneProps) {
  const { snapshot, pool } = scene;
  return (
    <g data-pool>
      <g pointerEvents="none">
        <text x={G.memX0} y={16} className={TZ}>
          Mempool
        </text>
        <text x={G.memX0} y={30} className={T10}>
          transaction pool
        </text>
        <line
          x1={G.memX0}
          x2={G.memX0 + G.memCols * U - 1}
          y1={G.propLblY - 13}
          y2={G.propLblY - 13}
          strokeWidth={1}
          vectorEffect="non-scaling-stroke"
          style={{ stroke: tok('border-subtle') }}
        />
      </g>
      {pool ? (
        <>
          <text x={G.memX0} y={G.pendLblY} className={T10B} pointerEvents="none">
            {pool.pending.total ? `pending · ${fmtInt(pool.pending.total)}` : 'pending · none'}
          </text>
          <text x={G.memX0} y={G.propLblY} className={T10B} pointerEvents="none">
            {`proposed · ${fmtInt(pool.proposed.total)}`}
          </text>
          <Grid grid={pool.pending} top={G.pendTop} dimOf={dimOf} onIds={onIds} testId="pending" />
          <Grid
            grid={pool.proposed}
            top={G.propPoolTop}
            dimOf={dimOf}
            onIds={onIds}
            testId="proposed"
          />
        </>
      ) : (
        <text data-pool-status x={G.memX0} y={G.pendLblY} className={T10B}>
          <title>
            {`The node's tx pool is at #${fmtInt(snapshot.poolTip.number)} (${snapshot.poolTip.hash}) but ckbadger has synced #${fmtInt(snapshot.tip.number)} (${snapshot.tip.hash}). The pool view returns once both are at the same block.`}
          </title>
          pool ≠ synced tip
        </text>
      )}
    </g>
  );
}
