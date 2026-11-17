'use client';

import { useId } from 'react';
import { color } from '@/lib/palette';
import {
  CUT_FILL,
  GHOST,
  PLATE_EDGE,
  PLATE_FILL,
  WELL_FILL,
  agedChip,
  diamondPath,
} from '@/lib/pipeline/encoding';
import {
  COL,
  G,
  cardBox,
  columnChipCountAt,
  columnCountAt,
  columnFlowPaths,
  columnHeadAt,
  columnWellCountAt,
  columnWellHeadAt,
  columnWellRect,
  columnWellShade,
  tail4,
  type Band,
  type Scene,
  type SceneCard,
  type SceneNext,
  type ScenePoolGrid,
} from '@/lib/pipeline/layout';
import { PlateStone, type DimLevel } from '@/components/pipeline/block-card';
import { FlowLink } from '@/components/pipeline/pool-zone';
import { T10, T10B, T11C, T12C, T12D, T12G, fmtInt } from '@/components/pipeline/svg-text';
import { TxSquare } from '@/components/pipeline/tx-square';
import { formatBlockNumber } from '@/lib/format-block';

/**
 * The rows of the column orientation (the pipeline stood on end, `COL` in lib/pipeline/layout.ts;
 * PlatePhone.dc.html). The same pieces as the row, from the same library, at the same 17 px pitch:
 * a pool a square well, NEXT the one dashed outline, a block a solid plate with its stone, its
 * dots and its tx line cut into it. Every row carries the same data attributes as its card in a
 * row, so the motion layer, the hover and the pin treat both orientations alike; only the
 * arrangement differs. Every number is the scene's or the library's.
 */

/** A word on the middle of an uncle's own line (its word and its "+N"): the line's baseline. */
const wordY = (u: { y: number; h: number }) => u.y + u.h / 2 + 3.5;

interface ColumnBlockProps {
  card: SceneCard;
  /** The block number is gold while it lies inside the drawn proposal window. */
  numberInWindow: boolean;
  highlighted: boolean;
  dim: DimLevel;
  onIds?: ReadonlySet<string>;
  /** Blocks behind the tip (0 = the tip): an older block's marks step back by their fill. */
  age: number;
}

/**
 * A mined block as a row: a solid plate across the canvas, of its own tone, no stroke at rest (a
 * 1 px edge in the dim ink while it is highlighted), never faded, as `BlockCard`. On the middle of
 * its tx line (`columnHeadAt`, never of a row grown by its uncles' own line) its stone and its
 * number, its own proposal dots, then the line: the cellbase and its txs, each set in its cut —
 * one cut per mark, drawn first, and none where there is no transaction — then its "+N" and its
 * uncles (each its word and its dots or its "+N"; no box, no stone), on the line or on a line of
 * their own under it.
 *
 * An older block steps back by its marks' fill alone (`agedPaint` toward the cut, `agedChip`
 * toward the plate); the plate, the stone and the words keep full strength, the number in
 * `text-bright`, "uncle" and every "+N" in `text`. The words let the pointer through to the plate
 * and the marks beneath them, but an uncle's word, which is what the pointer reads the uncle by.
 * The motion layer owns the outer group's `opacity` and, while a block lands, its box.
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
  const head = columnHeadAt(card);
  const chipCount = columnChipCountAt(card);
  const count = columnCountAt(card);
  return (
    <g
      data-card={card.id}
      data-number={block.number}
      data-slot={card.slot}
      data-age={age}
      transform={`translate(${card.x},${card.y})`}
      className="cursor-pointer"
    >
      <g data-dim={dim}>
        <rect
          data-box
          {...cardBox('column', card.w, card.h, highlighted)}
          strokeWidth={1}
          vectorEffect="non-scaling-stroke"
          fillOpacity={1}
          // the whole plate is what a pointer reads the block by, even while it is an outline
          pointerEvents="all"
          style={{ fill: PLATE_FILL, stroke: highlighted ? PLATE_EDGE : 'none' }}
        />
        {card.stone && (
          <PlateStone hash={block.hash} txs={block.txCount} x={head.stoneX} y={head.stoneY} />
        )}
        <g data-cuts>
          {card.cuts.map((c) => (
            <rect
              key={c.id}
              data-cut={c.id}
              x={c.x}
              y={c.y}
              width={c.s}
              height={c.s}
              style={{ fill: CUT_FILL }}
            />
          ))}
        </g>
        <g data-field>
          {card.squares.map((sq) => (
            <TxSquare key={sq.id} sq={sq} age={age} on={onIds?.has(sq.id)} />
          ))}
        </g>
        <g data-chips>
          {card.chips.map((k) => {
            const paint = agedChip(k.proposal.state, age);
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
        </g>
        <g data-words pointerEvents="none">
          <text
            data-block-number
            x={card.numberX}
            y={head.y}
            className={numberInWindow ? T12G : T12C}
          >
            {tail4(block.number)}
          </text>
          {card.uncles.map((u) => (
            <g key={u.ref.hash} data-uncle={u.index} pointerEvents="visiblePainted">
              <text x={u.x} y={wordY(u)} className={T10B}>
                {u.label}
              </text>
              {u.overflow > 0 && (
                <text
                  x={u.x + u.w}
                  y={wordY(u)}
                  textAnchor="end"
                  className={T10B}
                >{`+${u.overflow}`}</text>
              )}
            </g>
          ))}
          {card.chipOverflow > 0 && (
            <g data-chips-label>
              <text
                data-chip-overflow
                x={chipCount.x}
                y={chipCount.y}
                textAnchor="end"
                className={T10B}
              >{`+${card.chipOverflow}`}</text>
            </g>
          )}
          {card.hidden > 0 && (
            <text data-count x={count.x} y={count.y} textAnchor="end" className={T10B}>
              {`+${card.hidden}`}
            </text>
          )}
        </g>
      </g>
    </g>
  );
}

/**
 * The predicted next block as a row: no plate yet, so no stone, only its dashed outline at the
 * row's size (the canvas's only dashed outline), its number in the dim ink, and on its line what
 * the node expects to commit, as dashed ghosts at the plate's unit.
 */
export function ColumnNext({ next, dim }: { next: SceneNext; dim: DimLevel }) {
  const shape = { w: next.w, h: next.h, uncles: [] };
  const head = columnHeadAt(shape);
  const chipCount = columnChipCountAt(shape);
  const count = columnCountAt(shape);
  return (
    <g data-next transform={`translate(${next.x},${next.y})`} className="cursor-pointer">
      <g data-dim={dim}>
        <rect
          data-box
          {...cardBox('column', next.w, next.h, true)}
          strokeWidth={1}
          strokeDasharray={GHOST.outline.dash}
          vectorEffect="non-scaling-stroke"
          strokeOpacity={GHOST.outline.strokeOpacity}
          // no fill, yet the whole row is what a pointer reads NEXT by
          pointerEvents="all"
          style={{ fill: 'none', stroke: GHOST.outline.stroke }}
        />
        <text x={COL.nameX - COL.x0} y={head.y} className={T12D}>
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
  const at = columnWellHeadAt(band);
  return (
    <text data-next-column x={at.nameX} y={at.y} className={T10} pointerEvents="none">
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
 * A pool stage as a row: a well, never a block (PlatePhone.dc.html `.rw.wl`). Square, recessed
 * below the page under a 1 px edge in the line ink, the theme's inset a 3 px shade under that
 * edge, fading to nothing, as the row's well. In it its name in the stage's colour, its pool total
 * ending at `countX`, its txs loose on the line (a well does not age and cuts nothing) and a "+N"
 * for those the line cannot hold.
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
  const shade = `pipeline-inset${useId().replace(/[^\w-]/g, '')}`;
  const head = columnWellHeadAt(band);
  const more = columnWellCountAt(scene.viewW, band);
  const { snapshot } = scene;
  return (
    <g data-pool-card={testId}>
      <rect
        data-tray
        {...columnWellRect(scene.viewW, band)}
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
      <rect
        data-inset
        {...columnWellShade(scene.viewW, band)}
        fill={`url(#${shade})`}
        pointerEvents="none"
      />
      <text x={head.nameX} y={head.y} className={T11C} pointerEvents="none">
        <tspan className={tone === 'aqua' ? 'fill-aqua' : 'fill-gold'}>{title}</tspan>
      </text>
      <text
        data-count
        x={head.countX}
        y={head.y}
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
              <TxSquare sq={sq} age={null} on={onIds?.has(sq.id)} />
            </g>
          ))}
        </g>
      ) : (
        testId === 'pending' && (
          <text data-pool-status x={COL.lineX} y={head.y} className={T10B}>
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

/**
 * The tx lifetime starts at the top: the mempool row, then the proposed row, then NEXT's; a faint
 * band of ticks runs down each gap between them (`columnFlowPaths`), as the row's flows.
 */
export function ColumnPool({ scene, dimOf, onIds }: ColumnPoolProps) {
  const { bands, pool } = scene;
  if (!bands) throw new Error('pipeline: a column scene has no bands');
  const flows = columnFlowPaths(bands);
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
      <FlowLink d={flows.toProposed} />
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
      <FlowLink d={flows.toNext} />
    </g>
  );
}
