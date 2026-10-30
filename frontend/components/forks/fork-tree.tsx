'use client';

import { useId } from 'react';
import { FORK_COLORS } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import { blockClock } from '@/lib/fork-format';
import type { ForkTreeModel, ForkTreeTile } from '@/lib/fork-tree';
import { formatUtc8DateTime } from '@/lib/utils';

const CW = 112;
const TW = 96;
const TH = 58;
const X0 = 30;
const GAP = 46;

const MARK = ['', '①', '②'] as const;

/**
 * "The two branches" (Fork board V67): today's chain on the main row, the branch that lost dashed
 * above it, a jade arrow into the block that took a dropped block as an uncle, and ① ② on the
 * tips this node had before and after the switch.
 */
export function ForkTree({ model }: { model: ForkTreeModel }) {
  const marker = useId().replace(/:/g, '');
  const k = model.rows.length;
  const yMain = 46 + k * (TH + GAP);
  const rowY = (i: number) => yMain - (i + 1) * (TH + GAP);
  const x = (n: number) => X0 + (n - model.first) * CW;
  const width = x(model.last) + TW + 30;
  const height = yMain + TH + 30;
  const mainByNumber = new Map(model.main.map((tile) => [tile.number, tile]));
  const targets = new Set(model.uncleLinks.map((link) => link.to));
  const position = new Map<string, { x: number; y: number; row: number }>();
  model.main.forEach((tile) => position.set(tile.hash, { x: x(tile.number), y: yMain, row: -1 }));
  model.rows.forEach((row, i) =>
    row.tiles.forEach((tile) => position.set(tile.hash, { x: x(tile.number), y: rowY(i), row: i }))
  );

  const tile = (t: ForkTreeTile, y: number, dropped: boolean) => {
    const tx = x(t.number);
    const lit = !dropped && (t.number === model.main[0].number || targets.has(t.number));
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
          rx="4"
          fill={FORK_COLORS.tileFill}
          stroke={
            dropped ? FORK_COLORS.dropped : lit ? FORK_COLORS.tileStrokeLit : FORK_COLORS.tileStroke
          }
          strokeDasharray={dropped ? '3 3' : undefined}
        />
        <text x={tx + 8} y={y + 16} fontSize="11" fill={FORK_COLORS.ink}>
          #{String(t.number % 1000).padStart(3, '0')}
        </text>
        <text x={tx + TW - 8} y={y + 16} textAnchor="end" fontSize="10" fill={FORK_COLORS.tick}>
          {t.hash.slice(2, 8)}
        </text>
        {t.timestamp && (
          <text x={tx + 8} y={y + 32} fontSize="10" fill={FORK_COLORS.dim}>
            {blockClock(t.timestamp)}
          </text>
        )}
        {t.minerAddress && (
          <text x={tx + 8} y={y + 47} fontSize="10" fill={FORK_COLORS.tick}>
            …{t.minerAddress.slice(-6)}
          </text>
        )}
      </g>
    );
  };

  return (
    <svg
      data-fork-tree
      viewBox={`0 0 ${width} ${height}`}
      width="100%"
      role="img"
      aria-label={`Blocks ${formatBlockNumber(model.first)} to ${formatBlockNumber(model.last)}: today's chain, the branch that lost above it, and where it was taken as an uncle`}
      className="block min-w-[720px] overflow-visible"
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
        mainByNumber.has(t.number + 1) ? (
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
        const first = row.tiles[0];
        const parent = mainByNumber.get(first.number - 1);
        const y = rowY(i);
        return (
          <g key={row.label}>
            {parent && (
              <path
                d={`M${x(parent.number) + TW} ${yMain + 20} C${x(first.number) - 6} ${yMain + 20} ${x(first.number) - 14} ${y + TH / 2} ${x(first.number)} ${y + TH / 2}`}
                fill="none"
                stroke={FORK_COLORS.tick}
                strokeDasharray="3 3"
              />
            )}
            {row.tiles.slice(1).map((t, j) => (
              <line
                key={`link-${t.hash}`}
                x1={x(row.tiles[j].number) + TW}
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
      {model.main.map((t) => tile(t, yMain, false))}
      {model.rows.map((row, i) => row.tiles.map((t) => tile(t, rowY(i), true)))}
      {model.uncleLinks.map((link, i) => {
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
      {model.marks.map((mark) => {
        const at = position.get(mark.hash)!;
        const cx = at.row < 0 ? at.x + 4 : at.x + TW - 4;
        const cy = at.row < 0 ? at.y + TH + 2 : at.y - 2;
        return (
          <g key={mark.n} data-tip-mark={mark.n}>
            <circle
              cx={cx}
              cy={cy}
              r="9"
              fill={FORK_COLORS.halo}
              stroke={FORK_COLORS.ink}
              strokeWidth="1.2"
            />
            <text x={cx} y={cy + 4} textAnchor="middle" fontSize="11" fill={FORK_COLORS.ink}>
              {mark.n}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

export { MARK as TIP_MARKS };
