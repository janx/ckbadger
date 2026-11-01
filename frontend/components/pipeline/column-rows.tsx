'use client';

import {
  GHOST,
  TILE_FILL,
  WELL_FILL,
  ageOpacity,
  chipPaint,
  diamondPath,
  tok,
} from '@/lib/pipeline/encoding';
import {
  COL,
  G,
  columnChipCountAt,
  columnCountAt,
  columnWellCountAt,
  tail4,
  type Band,
  type Scene,
  type SceneCard,
  type SceneNext,
  type ScenePoolGrid,
} from '@/lib/pipeline/layout';
import type { DimLevel } from '@/components/pipeline/block-card';
import { T10, T10B, T11C, T11G, fmtInt } from '@/components/pipeline/svg-text';
import { TxSquare } from '@/components/pipeline/tx-square';
import { formatBlockNumber } from '@/lib/format-block';

/**
 * The rows of the column orientation (the pipeline stood on end, `COL` in lib/pipeline/layout.ts).
 * Every row carries the same data attributes as its card in a row, so the motion layer, the hover
 * and the pin treat both orientations alike; only the arrangement differs. The materials are the
 * row's: recessed wells, NEXT the one dashed outline, blocks as tiles fading with age.
 */

/** Baselines on a row's centre line: 11 px names and numbers, 10 px counts. */
const nameY = (h: number) => h / 2 + 4;
const countY = (h: number) => h / 2 + 3.5;
/** The name or number of a row, local to a row card. */
const NAME_X = COL.nameX - COL.x0;

function Dots({ chips }: { chips: SceneCard['chips'] }) {
  return (
    <>
      {chips.map((k) => {
        const paint = chipPaint(k.proposal.state);
        return (
          <circle
            key={k.key}
            data-chip={k.key}
            data-state={k.proposal.state}
            data-uncle-index={k.proposal.uncleIndex ?? undefined}
            cx={k.x + G.chip / 2}
            cy={k.y + G.chip / 2}
            r={paint.stroke ? G.chip / 2 - 0.5 : G.chip / 2}
            fillOpacity={paint.fillOpacity}
            strokeWidth={paint.stroke ? 1 : undefined}
            vectorEffect="non-scaling-stroke"
            style={{ fill: paint.fill, stroke: paint.stroke ?? 'none' }}
          />
        );
      })}
    </>
  );
}

interface ColumnBlockProps {
  card: SceneCard;
  /** The block number is gold while it lies inside the drawn proposal window. */
  numberInWindow: boolean;
  highlighted: boolean;
  dim: DimLevel;
  onIds?: ReadonlySet<string>;
  /** Blocks behind the tip (0 = the tip). */
  age: number;
}

/**
 * A mined block as a row: its number, its own proposal dots, then its tx line from the cellbase,
 * with "+N" and its uncle boxes at the end of the line.
 */
export function ColumnBlock({
  card,
  numberInWindow,
  highlighted,
  dim,
  onIds,
  age,
}: ColumnBlockProps) {
  const { block } = card;
  const chipCount = columnChipCountAt(card.h);
  const count = columnCountAt(card);
  const u = COL.uncle;
  return (
    <g
      data-card={card.id}
      data-number={block.number}
      data-slot={card.slot}
      transform={`translate(${card.x},${card.y})`}
      className="cursor-pointer"
    >
      <g data-age={age} style={{ opacity: ageOpacity(age) }}>
        <g data-dim={dim}>
          <rect
            data-box
            x={0.5}
            y={0.5}
            width={Math.max(1, card.w - 1)}
            height={Math.max(1, card.h - 1)}
            rx={3}
            strokeWidth={1}
            vectorEffect="non-scaling-stroke"
            fillOpacity={1}
            strokeOpacity={highlighted ? 0.7 : 0}
            style={{ fill: TILE_FILL, stroke: tok('jade') }}
          />
          <text
            data-block-number
            x={NAME_X}
            y={nameY(card.h)}
            className={numberInWindow ? T11G : T11C}
          >
            {tail4(block.number)}
          </text>
          <g data-uncles>
            {card.uncles.map((un) => (
              <g key={un.ref.hash} data-uncle={un.index}>
                <rect
                  x={un.x}
                  y={un.y}
                  width={un.w}
                  height={un.h}
                  rx={3}
                  style={{ fill: tok('surface') }}
                />
                <text x={un.x + u.pad} y={un.y + countY(un.h)} className={T10}>
                  {un.label}
                </text>
                {un.overflow > 0 && (
                  <text
                    x={un.x + un.w - u.pad}
                    y={un.y + countY(un.h)}
                    textAnchor="end"
                    className={T10}
                  >{`+${un.overflow}`}</text>
                )}
              </g>
            ))}
          </g>
          <g data-chips>
            <Dots chips={card.chips} />
            {card.chipOverflow > 0 && (
              <text
                data-chip-overflow
                x={chipCount.x}
                y={chipCount.y}
                textAnchor="end"
                className={T10}
              >{`+${card.chipOverflow}`}</text>
            )}
          </g>
          <g data-field>
            {card.squares.map((sq) => (
              <TxSquare key={sq.id} sq={sq} on={onIds?.has(sq.id)} />
            ))}
          </g>
          {card.hidden > 0 && (
            <text data-count x={count.x} y={count.y} textAnchor="end" className={T10}>
              {`+${card.hidden}`}
            </text>
          )}
        </g>
      </g>
    </g>
  );
}

/** The predicted next block as a row: dashed, because none of it is real yet. */
export function ColumnNext({ next, dim }: { next: SceneNext; dim: DimLevel }) {
  const chipCount = columnChipCountAt(next.h);
  const count = columnCountAt({ w: next.w, h: next.h, uncles: [] });
  return (
    <g data-next transform={`translate(${next.x},${next.y})`} className="cursor-pointer">
      <g data-dim={dim}>
        <rect
          data-box
          x={0.5}
          y={0.5}
          width={Math.max(1, next.w - 1)}
          height={Math.max(1, next.h - 1)}
          rx={3}
          strokeWidth={1}
          strokeDasharray="3 2"
          vectorEffect="non-scaling-stroke"
          fillOpacity={0.5}
          strokeOpacity={0.6}
          style={{ fill: tok('elevated'), stroke: tok('jade') }}
        />
        <text x={NAME_X} y={nameY(next.h)} className={T11C}>
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
              x={chipCount.x}
              y={chipCount.y}
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
          <text data-count x={count.x} y={count.y} textAnchor="end" className={T10}>
            {`+${next.hidden}`}
          </text>
        )}
      </g>
    </g>
  );
}

/** Nothing labels NEXT's row (its dashed outline says "predicted"); only a missing one is said. */
export function ColumnNextUnavailable({ available, band }: { available: boolean; band: Band }) {
  if (available) return null;
  return (
    <text
      data-next-column
      x={COL.nameX}
      y={band.y + countY(band.h)}
      className={T10}
      pointerEvents="none"
    >
      unavailable
    </text>
  );
}

interface ColumnPoolProps {
  scene: Scene;
  dimOf?: (hash: string) => DimLevel;
  onIds?: ReadonlySet<string>;
}

/**
 * A pool stage as a recessed row, never a tile: its name in the stage's colour, its pool total
 * ending at `countX`, its txs on the line and a "+N" for those the line cannot hold.
 */
function ColumnWell({
  scene,
  band,
  title,
  tone,
  grid,
  testId,
  dimOf,
  onIds,
}: {
  scene: Scene;
  band: Band;
  title: string;
  tone: 'aqua' | 'gold';
  grid: ScenePoolGrid | null;
  testId: 'pending' | 'proposed';
} & Omit<ColumnPoolProps, 'scene'>) {
  const more = columnWellCountAt(scene.viewW, band);
  const { snapshot } = scene;
  return (
    <g data-pool-card={testId}>
      <rect
        data-tray
        x={COL.x0}
        y={band.y}
        width={scene.viewW - 2 * COL.x0}
        height={band.h}
        rx={3}
        stroke="none"
        style={{ fill: WELL_FILL }}
      />
      <text x={COL.nameX} y={band.y + nameY(band.h)} className={T11C} pointerEvents="none">
        <tspan className={tone === 'aqua' ? 'fill-aqua' : 'fill-gold'}>{title}</tspan>
      </text>
      <text
        data-count
        x={COL.countX}
        y={band.y + countY(band.h)}
        textAnchor="end"
        className={T10}
        pointerEvents="none"
      >
        {grid ? fmtInt(grid.total) : '—'}
      </text>
      {grid ? (
        <g data-pool-grid={testId}>
          {grid.squares.map((sq) => (
            <g key={sq.id} data-dim={dimOf ? dimOf(sq.id) : 'false'}>
              <TxSquare sq={sq} on={onIds?.has(sq.id)} />
            </g>
          ))}
        </g>
      ) : (
        testId === 'pending' && (
          <text data-pool-status x={COL.lineX} y={band.y + countY(band.h)} className={T10B}>
            <title>
              {`The node's tx pool is at ${formatBlockNumber(snapshot.poolTip.number)} (${snapshot.poolTip.hash}) but ckbadger has synced ${formatBlockNumber(snapshot.tip.number)} (${snapshot.tip.hash}). The pool view returns once both are at the same block.`}
            </title>
            unavailable
          </text>
        )
      )}
      {grid && grid.hidden > 0 && (
        <text
          data-hidden-count
          x={more.x}
          y={more.y}
          textAnchor="end"
          className={T10}
          pointerEvents="none"
        >{`+${fmtInt(grid.hidden)}`}</text>
      )}
    </g>
  );
}

/** The dashed link down a gap (no arrowhead): the tx moves on to the next stage. */
function ColumnFlow({ y0, y1 }: { y0: number; y1: number }) {
  return (
    <path
      data-flow
      d={`M${COL.chainX} ${y0}V${y1}`}
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

/** The tx lifetime starts at the top: the mempool row, then the proposed row, then NEXT's. */
export function ColumnPool({ scene, dimOf, onIds }: ColumnPoolProps) {
  const { bands, pool } = scene;
  if (!bands) throw new Error('pipeline: a column scene has no bands');
  return (
    <g data-pool>
      <ColumnWell
        scene={scene}
        band={bands.pending}
        title="mempool"
        tone="aqua"
        grid={pool?.pending ?? null}
        testId="pending"
        dimOf={dimOf}
        onIds={onIds}
      />
      <ColumnFlow y0={bands.pending.y + bands.pending.h} y1={bands.proposed.y} />
      <ColumnWell
        scene={scene}
        band={bands.proposed}
        title="proposed"
        tone="gold"
        grid={pool?.proposed ?? null}
        testId="proposed"
        dimOf={dimOf}
        onIds={onIds}
      />
      <ColumnFlow y0={bands.proposed.y + bands.proposed.h} y1={bands.next.y} />
    </g>
  );
}
