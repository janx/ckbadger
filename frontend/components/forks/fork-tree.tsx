'use client';

import { useId } from 'react';
import { FORK_COLORS } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import { blockClock } from '@/lib/fork-format';
import {
  forkTreeLayout,
  type ForkTreeLayout,
  type ForkTreeModel,
  type ForkTreeTile,
} from '@/lib/fork-tree';
import { useElementWidth } from '@/hooks/useElementWidth';
import { formatUtc8DateTime } from '@/lib/utils';
import { minerName } from '@/components/forks/miner-link';

const MARK = ['', '①', '②', '③'] as const;
/** A later mark on a block that already carries one is drawn this far beside it. */
const MARK_STEP = { wide: 22, compact: 18 } as const;
/** One character of 10 px JetBrains Mono, for fitting a miner name under a narrow tile. */
const CHAR_10 = 6.4;

/**
 * "The two branches" (Fork board V67): today's chain on the main row, the branch that lost dashed
 * above it, a jade arrow into the block that took a dropped block as an uncle, and ① ② ③ on the
 * tips this node had before and after the switch, and after a later switch reversed it.
 *
 * Responsive (V4): laid out for the measured column (`forkTreeLayout`). The desktop tree is kept
 * wherever it fits; narrower, tiles shrink to no less than 72 px with time and miner under them,
 * and a deep fork folds its oldest heights into one "+N earlier" column. Nothing scrolls.
 */
export function ForkTree({ model }: { model: ForkTreeModel }) {
  const [box, measured] = useElementWidth<HTMLDivElement>();
  return (
    <div ref={box} className="min-w-0">
      {measured !== null && (
        <Tree model={model} layout={forkTreeLayout(model, Math.floor(measured))} />
      )}
    </div>
  );
}

function Tree({ model, layout }: { model: ForkTreeModel; layout: ForkTreeLayout }) {
  const marker = useId().replace(/:/g, '');
  const { tileW: TW, tileH: TH, yMain, x, collapsed, labelsUnder } = layout;
  const compact = layout.form === 'compact';
  const shown = (n: number) => collapsed === null || n > collapsed.last;
  const mainByNumber = new Map(model.main.map((tile) => [tile.number, tile]));
  const targets = new Set(model.uncleLinks.map((link) => link.to));
  // A folded block is drawn in the "+N earlier" column on the main row.
  const position = new Map<string, { x: number; y: number; row: number }>();
  model.main.forEach((tile) => position.set(tile.hash, { x: x(tile.number), y: yMain, row: -1 }));
  model.rows.forEach((row, i) =>
    row.tiles.forEach((tile) =>
      position.set(
        tile.hash,
        shown(tile.number)
          ? { x: x(tile.number), y: layout.rowY[i], row: i }
          : { x: x(tile.number), y: yMain, row: -1 }
      )
    )
  );
  const minerChars = Math.floor((TW + 16) / CHAR_10);

  const tile = (t: ForkTreeTile, y: number, dropped: boolean) => {
    const tx = x(t.number);
    const lit = !dropped && (t.number === model.main[0].number || targets.has(t.number));
    const miner = t.minerAddress ? minerName(t.minerAddress, t.minerPool) : null;
    return (
      <g key={t.hash} data-tile={t.number} data-dropped={dropped || undefined}>
        <title>
          {[
            formatBlockNumber(t.number),
            t.hash,
            t.timestamp && formatUtc8DateTime(t.timestamp),
            t.transactionsCount !== null && `${t.transactionsCount} tx`,
            t.minerAddress && `miner ${t.minerAddress}`,
          ]
            .filter(Boolean)
            .join(' · ')}
        </title>
        <rect
          x={tx}
          y={y}
          width={TW}
          height={TH}
          rx={compact ? 3 : 4}
          fill={FORK_COLORS.tileFill}
          stroke={
            dropped ? FORK_COLORS.dropped : lit ? FORK_COLORS.tileStrokeLit : FORK_COLORS.tileStroke
          }
          strokeDasharray={dropped ? '3 3' : undefined}
        />
        <text
          x={tx + (compact ? 6 : 8)}
          y={y + (compact ? 14 : 16)}
          fontSize="11"
          fill={FORK_COLORS.ink}
        >
          #{String(t.number % 1000).padStart(3, '0')}
        </text>
        {compact ? (
          <text x={tx + 6} y={y + 27} fontSize="10" fill={FORK_COLORS.tick}>
            {t.hash.slice(2, 8)}
          </text>
        ) : (
          <text x={tx + TW - 8} y={y + 16} textAnchor="end" fontSize="10" fill={FORK_COLORS.tick}>
            {t.hash.slice(2, 8)}
          </text>
        )}
        {t.timestamp && (
          <text
            data-time
            x={labelsUnder ? tx : tx + 8}
            y={labelsUnder ? y + TH + 18 : y + 32}
            fontSize="10"
            fill={FORK_COLORS.dim}
          >
            {blockClock(t.timestamp)}
          </text>
        )}
        {miner && (
          <text
            x={labelsUnder ? tx : tx + 8}
            y={labelsUnder ? y + TH + 30 : y + 47}
            fontSize="10"
            fill={FORK_COLORS.tick}
          >
            {labelsUnder && miner.length > minerChars
              ? `${miner.slice(0, minerChars - 1)}…`
              : miner}
          </text>
        )}
      </g>
    );
  };

  const folded = collapsed
    ? [...model.main, ...model.rows.flatMap((row) => row.tiles)].filter((t) => !shown(t.number))
    : [];
  return (
    <svg
      data-fork-tree
      viewBox={`0 0 ${layout.width} ${layout.height}`}
      width={layout.width}
      height={layout.height}
      role="img"
      aria-label={`Blocks ${formatBlockNumber(model.first)} to ${formatBlockNumber(model.last)}: today's chain, the branch that lost above it, and where it was taken as an uncle`}
      className="block max-w-none overflow-visible"
    >
      <defs>
        <marker
          id={marker}
          viewBox="0 0 6 6"
          refX="5"
          refY="3"
          markerWidth="6"
          markerHeight="6"
          orient="auto"
        >
          <path d="M0 0L6 3L0 6Z" fill={FORK_COLORS.uncleLink} />
        </marker>
      </defs>
      {model.main.map((t) =>
        mainByNumber.has(t.number + 1) && shown(t.number + 1) ? (
          <line
            key={`spine-${t.number}`}
            x1={x(t.number) + TW}
            x2={x(t.number + 1)}
            y1={yMain + TH / 2}
            y2={yMain + TH / 2}
            stroke={FORK_COLORS.edge}
          />
        ) : null
      )}
      {model.rows.map((row, i) => {
        const tiles = row.tiles.filter((t) => shown(t.number));
        if (tiles.length === 0) return null;
        const first = tiles[0];
        const y = layout.rowY[i];
        // from the parent block, or from the "+N earlier" column when the row's start is folded
        const from = first === row.tiles[0] ? mainByNumber.get(first.number - 1) : model.main[0];
        const fromY = yMain + (compact ? 10 : 20);
        return (
          <g key={row.label}>
            {from && (
              <path
                d={`M${x(from.number) + TW} ${fromY} C${x(first.number) - 6} ${fromY} ${x(first.number) - 14} ${y + TH / 2} ${x(first.number)} ${y + TH / 2}`}
                fill="none"
                stroke={FORK_COLORS.tick}
                strokeDasharray="3 3"
              />
            )}
            {tiles.slice(1).map((t, j) => (
              <line
                key={`link-${t.hash}`}
                x1={x(tiles[j].number) + TW}
                x2={x(t.number)}
                y1={y + TH / 2}
                y2={y + TH / 2}
                stroke={FORK_COLORS.tick}
                strokeDasharray="3 3"
              />
            ))}
            <text x={x(first.number)} y={y - 8} fontSize="11" fill={FORK_COLORS.tick}>
              {row.label === 'dropped at this switch' ? 'branch' : 'adopted then, dropped since'}
            </text>
          </g>
        );
      })}
      <text x={x(model.first)} y={yMain - 8} fontSize="11" fill={FORK_COLORS.tick}>
        main chain
      </text>
      {collapsed && (
        <g data-collapsed>
          <title>
            {`${collapsed.count} earlier heights, ${formatBlockNumber(collapsed.first)} to ${formatBlockNumber(collapsed.last)}: ${folded
              .map((t) => `${formatBlockNumber(t.number)} ${t.hash.slice(0, 10)}`)
              .join(', ')}`}
          </title>
          <rect
            x={x(collapsed.first)}
            y={yMain}
            width={TW}
            height={TH}
            rx="3"
            fill={FORK_COLORS.tileFill}
            stroke={FORK_COLORS.tileStroke}
          />
          <text x={x(collapsed.first) + 6} y={yMain + 14} fontSize="11" fill={FORK_COLORS.ink}>
            +{collapsed.count} earlier
          </text>
          <text x={x(collapsed.first) + 6} y={yMain + 27} fontSize="10" fill={FORK_COLORS.tick}>
            #{String(collapsed.first % 1000).padStart(3, '0')}–#
            {String(collapsed.last % 1000).padStart(3, '0')}
          </text>
        </g>
      )}
      {model.main.filter((t) => shown(t.number)).map((t) => tile(t, yMain, false))}
      {model.rows.map((row, i) =>
        row.tiles.filter((t) => shown(t.number)).map((t) => tile(t, layout.rowY[i], true))
      )}
      {model.uncleLinks
        .filter((link) => shown(link.to))
        .map((link, i) => {
          const from = position.get(link.from)!;
          const x0 = from.x + TW / 2;
          const x1 = x(link.to) + TW / 2 + (i % 2 ? 8 : -8);
          return (
            <path
              key={`${link.from}-${link.to}`}
              data-uncle-link
              d={`M${x0} ${from.y} C${x0} ${from.y - 30} ${x1} ${from.y - 34} ${x1} ${yMain - 2}`}
              fill="none"
              stroke={FORK_COLORS.uncleLink}
              strokeWidth="1.2"
              markerEnd={`url(#${marker})`}
            />
          );
        })}
      {model.marks.map((mark, i) => {
        const at = position.get(mark.hash)!;
        const before = model.marks.slice(0, i).filter((m) => m.hash === mark.hash).length;
        const main = at.row < 0;
        const r = compact ? 8 : 9;
        let cx: number;
        let cy: number;
        if (compact) {
          // on the box's top-right corner on either row: under a box hang its time and miner, and
          // the uncle arcs land at its top centre; a second mark on the same block steps left
          cx = at.x + TW - 6 - before * MARK_STEP.compact;
          cy = at.y - 2;
        } else {
          cx = (main ? at.x + 4 : at.x + TW - 4) + before * MARK_STEP.wide * (main ? 1 : -1);
          cy = main ? at.y + TH + 2 : at.y - 2;
        }
        return (
          <g key={mark.n} data-tip-mark={mark.n}>
            <circle
              cx={cx}
              cy={cy}
              r={r}
              fill={FORK_COLORS.halo}
              stroke={FORK_COLORS.ink}
              strokeWidth="1.2"
            />
            <text
              x={cx}
              y={cy + (compact ? 3.5 : 4)}
              textAnchor="middle"
              fontSize={compact ? 10 : 11}
              fill={FORK_COLORS.ink}
            >
              {mark.n}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

export { MARK as TIP_MARKS };
