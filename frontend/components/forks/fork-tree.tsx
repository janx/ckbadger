'use client';

import { useId, type ReactNode } from 'react';
import { FORK_COLORS } from '@/lib/design-tokens';
import { formatBlockNumber } from '@/lib/format-block';
import { blockClock } from '@/lib/fork-format';
import {
  FORK_TREE,
  LANE_NAMES,
  TILE_HASH_DIGITS,
  forkTreeLayout,
  type ForkTreeColumnLayout,
  type ForkTreeModel,
  type ForkTreeTile,
  type ForkTreeWideLayout,
} from '@/lib/fork-tree';
import { useElementWidth } from '@/hooks/useElementWidth';
import { formatUtc8DateTime } from '@/lib/utils';
import { minerName } from '@/components/forks/miner-link';
import { clipForeignLabel, type ShortSize } from '@/lib/shorten';
import { SVG_TEXT, centreShift } from '@/lib/chart-type';
import { descentPx, textPx } from '@/lib/type-metrics';

const MARK = ['', '①', '②', '③'] as const;
/** A later mark on a block that already carries one is drawn this far beside it (the wide tree). */
const MARK_STEP = 22;
/** A tip mark hangs this far over (or under) the tile corner it marks (the wide tree). */
const MARK_OVERHANG = 2;

/**
 * "The two branches" (Fork board V67): today's chain, the branch that lost dashed beside it, a jade
 * arrow into the block that took a dropped block as an uncle, and ① ② ③ on the tips this node had
 * before and after the switch, and after a later switch reversed it.
 *
 * Responsive (V4, lane P4): laid out for the measured column (`forkTreeLayout`). The desktop tree,
 * heights left to right, is kept wherever it fits; narrower, the tree turns to a column, one block
 * a row with its time and miner beside it. Nothing folds and nothing scrolls.
 */
export function ForkTree({ model }: { model: ForkTreeModel }) {
  const [box, measured] = useElementWidth<HTMLDivElement>();
  if (measured === null) return <div ref={box} className="min-w-0" />;
  const layout = forkTreeLayout(model, Math.floor(measured));
  return (
    <div ref={box} className="min-w-0">
      {layout.form === 'wide' ? (
        <WideTree model={model} layout={layout} />
      ) : (
        <ColumnTree model={model} layout={layout} />
      )}
    </div>
  );
}

/** The tree's svg, its arrowhead and its name for a reader that does not see it. */
function TreeSvg({
  model,
  width,
  height,
  marker,
  children,
}: {
  model: ForkTreeModel;
  width: number;
  height: number;
  marker: string;
  children: ReactNode;
}) {
  return (
    <svg
      data-fork-tree
      viewBox={`0 0 ${width} ${height}`}
      width={width}
      height={height}
      role="img"
      aria-label={`Blocks ${formatBlockNumber(model.first)} to ${formatBlockNumber(model.last)}: today's chain, the branch that lost beside it, and where it was taken as an uncle`}
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
      {children}
    </svg>
  );
}

/**
 * A tile's title: everything the API says of the block, the whole hash and miner among it — a
 * pool's whole name too, which the tile may cut to its room (PAGE-LANE-NOTES F3-e: a cut pool name
 * showed nowhere whole, the miner's link titled by its address alone).
 */
function TileTitle({ t }: { t: ForkTreeTile }) {
  return (
    <title>
      {[
        formatBlockNumber(t.number),
        t.hash,
        t.timestamp && formatUtc8DateTime(t.timestamp),
        t.transactionsCount !== null && `${t.transactionsCount} tx`,
        t.minerAddress &&
          `miner ${t.minerPool === null ? '' : `${t.minerPool}, `}${t.minerAddress}`,
      ]
        .filter(Boolean)
        .join(' · ')}
    </title>
  );
}

/**
 * A miner on the tree (review F-B, logic finding 4), in the room its tile's words have
 * (`layout.labelRoom`: inside a wide tile its width less the insets; in the column, from the words'
 * column to the canvas's edge): a pool by its name, a word in any script (the operator's
 * pools.toml), measured as text that is not ours (1.25 em a grapheme, the figures' measure, which
 * is not a bound: a clip at the room's edge holds what it misses) and cut to the room at whole
 * graphemes with the one ellipsis; an unnamed one by its address in the id form the room holds,
 * never cut again: the row form, the narrative's and the tables' (R9), where it stands whole (13
 * characters, 93.6 px at 12 px), else the tight form (11 characters, 79.2 px).
 */
function minerLabel(t: ForkTreeTile, room: number): string | null {
  if (!t.minerAddress) return null;
  if (t.minerPool !== null) return clipForeignLabel(t.minerPool, SVG_TEXT.tick, room);
  const address = t.minerAddress;
  const fits = (size: ShortSize) =>
    textPx(minerName(address, null, size).length, SVG_TEXT.tick) <= room;
  return minerName(address, null, fits('row') ? 'row' : 'tight');
}

/** A tile's stroke: dashed in the dropped ink, else lit on the fork point and an uncle's block. */
function tileStroke(dropped: boolean, lit: boolean) {
  return dropped ? FORK_COLORS.dropped : lit ? FORK_COLORS.tileStrokeLit : FORK_COLORS.tileStroke;
}

/** The tile's number on the tree: the last three digits of its height, `#176`. */
const tileNumber = (n: number) => `#${String(n % 1000).padStart(3, '0')}`;

function WideTree({ model, layout }: { model: ForkTreeModel; layout: ForkTreeWideLayout }) {
  const marker = useId().replace(/:/g, '');
  const { tileW: TW, tileH: TH, yMain, x } = layout;
  const mainByNumber = new Map(model.main.map((tile) => [tile.number, tile]));
  const targets = new Set(model.uncleLinks.map((link) => link.to));
  const position = new Map<string, { x: number; y: number; row: number }>();
  model.main.forEach((tile) => position.set(tile.hash, { x: x(tile.number), y: yMain, row: -1 }));
  model.rows.forEach((row, i) =>
    row.tiles.forEach((tile) =>
      position.set(tile.hash, { x: x(tile.number), y: layout.rowY[i], row: i })
    )
  );
  const W = FORK_TREE.wide;
  // A row's name stands over the tip marks that hang on its tiles' top corners, a pixel clear of
  // them: "adopted then, dropped since" is wider than a tile, and its row's tip is marked (②).
  const rowNameLift = W.markR + MARK_OVERHANG + 1 + descentPx(SVG_TEXT.label);

  const tile = (t: ForkTreeTile, y: number, dropped: boolean) => {
    const tx = x(t.number);
    const lit = !dropped && (t.number === model.main[0].number || targets.has(t.number));
    const miner = minerLabel(t, layout.labelRoom(t));
    return (
      <g key={t.hash} data-tile={t.number} data-dropped={dropped || undefined}>
        <TileTitle t={t} />
        <rect
          x={tx}
          y={y}
          width={TW}
          height={TH}
          fill={FORK_COLORS.tileFill}
          stroke={tileStroke(dropped, lit)}
          strokeDasharray={dropped ? '3 3' : undefined}
        />
        <text x={tx + W.inset} y={y + W.numberY} fontSize={SVG_TEXT.label} fill={FORK_COLORS.ink}>
          {tileNumber(t.number)}
        </text>
        <text
          x={tx + TW - W.inset}
          y={y + W.numberY}
          textAnchor="end"
          fontSize={SVG_TEXT.tick}
          fill={FORK_COLORS.tick}
        >
          {t.hash.slice(2, 2 + TILE_HASH_DIGITS)}
        </text>
        {t.timestamp && (
          <text
            data-time
            x={tx + W.inset}
            y={y + W.timeY}
            fontSize={SVG_TEXT.tick}
            fill={FORK_COLORS.dim}
          >
            {blockClock(t.timestamp)}
          </text>
        )}
        {miner && (
          <text x={tx + W.inset} y={y + W.minerY} fontSize={SVG_TEXT.tick} fill={FORK_COLORS.tick}>
            {miner}
          </text>
        )}
      </g>
    );
  };

  return (
    <TreeSvg model={model} width={layout.width} height={layout.height} marker={marker}>
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
        const tiles = row.tiles;
        if (tiles.length === 0) return null;
        const first = tiles[0];
        const y = layout.rowY[i];
        // from the parent block on today's chain
        const from = mainByNumber.get(first.number - 1);
        const fromY = yMain + 20;
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
            <text
              x={x(first.number)}
              y={y - rowNameLift}
              fontSize={SVG_TEXT.label}
              fill={FORK_COLORS.tick}
            >
              {LANE_NAMES[row.label]}
            </text>
          </g>
        );
      })}
      <text
        x={x(model.first)}
        y={yMain - rowNameLift}
        fontSize={SVG_TEXT.label}
        fill={FORK_COLORS.tick}
      >
        {LANE_NAMES.main}
      </text>
      {model.main.map((t) => tile(t, yMain, false))}
      {model.rows.map((row, i) => row.tiles.map((t) => tile(t, layout.rowY[i], true)))}
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
      {model.marks.map((mark, i) => {
        const at = position.get(mark.hash)!;
        const before = model.marks.slice(0, i).filter((m) => m.hash === mark.hash).length;
        const main = at.row < 0;
        const cx = (main ? at.x + 4 : at.x + TW - 4) + before * MARK_STEP * (main ? 1 : -1);
        const cy = main ? at.y + TH + MARK_OVERHANG : at.y - MARK_OVERHANG;
        return <TipMark key={mark.n} n={mark.n} cx={cx} cy={cy} r={W.markR} role="label" />;
      })}
    </TreeSvg>
  );
}

/** A tip mark, ① ② ③: its digit at the role its tree sets (the label in the wide tree, the tick in the column). */
function TipMark({
  n,
  cx,
  cy,
  r,
  role,
}: {
  n: 1 | 2 | 3;
  cx: number;
  cy: number;
  r: number;
  role: 'label' | 'tick';
}) {
  const size = SVG_TEXT[role];
  return (
    <g data-tip-mark={n}>
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
        y={cy + centreShift(size)}
        textAnchor="middle"
        fontSize={SVG_TEXT[role]}
        fill={FORK_COLORS.ink}
      >
        {n}
      </text>
    </g>
  );
}

/**
 * The phone form (lane P4): the tree turned to a column. One block a row in order of height, each
 * branch in a lane of its own (`layout.laneX`), named once over its lane; every tile's time and
 * miner beside it, on its row, in the column of words right of the lanes. The lines run only in
 * the lanes and the gaps between them: today's chain down its lane, the branch's dashed line from
 * its parent and down its own lane, an uncle's arrow in the gap between the first two lanes, from
 * the dropped block to the block that took it. A tip mark hangs over its tile's right corner.
 */
function ColumnTree({ model, layout }: { model: ForkTreeModel; layout: ForkTreeColumnLayout }) {
  const marker = useId().replace(/:/g, '');
  const C = FORK_TREE.column;
  const { tileW: TW, tileH: TH, laneX, labelsX } = layout;
  const mainByNumber = new Map(model.main.map((tile) => [tile.number, tile]));
  const targets = new Set(model.uncleLinks.map((link) => link.to));
  const linkX = (lane: number) => laneX[lane] + C.linkX;
  return (
    <TreeSvg model={model} width={layout.width} height={layout.height} marker={marker}>
      {layout.laneNames.map((name) => (
        <text
          key={name.text}
          data-lane-name
          x={name.x}
          y={name.y}
          textAnchor={name.anchor === 'end' ? 'end' : undefined}
          fontSize={SVG_TEXT.label}
          fill={FORK_COLORS.tick}
        >
          {name.text}
        </text>
      ))}
      {model.main.map((t) => {
        const next = mainByNumber.get(t.number + 1);
        if (!next) return null;
        return (
          <line
            key={`spine-${t.number}`}
            x1={linkX(0)}
            x2={linkX(0)}
            y1={layout.at(t.hash).y + TH}
            y2={layout.at(next.hash).y}
            stroke={FORK_COLORS.edge}
          />
        );
      })}
      {model.rows.map((row, i) => {
        const lane = i + 1;
        const tiles = row.tiles;
        if (tiles.length === 0) return null;
        const first = layout.at(tiles[0].hash);
        const parent = mainByNumber.get(tiles[0].number - 1);
        const from = parent && layout.at(parent.hash);
        const lx = linkX(lane);
        return (
          <g key={row.label}>
            {from && (
              // from the parent's right edge across to the branch's lane, then down into its tile
              <path
                d={`M${laneX[0] + TW} ${from.y + TH / 2} H${lx - 6} Q${lx} ${from.y + TH / 2} ${lx} ${from.y + TH / 2 + 6} V${first.y}`}
                fill="none"
                stroke={FORK_COLORS.tick}
                strokeDasharray="3 3"
              />
            )}
            {tiles.slice(1).map((t, j) => (
              <line
                key={`link-${t.hash}`}
                x1={lx}
                x2={lx}
                y1={layout.at(tiles[j].hash).y + TH}
                y2={layout.at(t.hash).y}
                stroke={FORK_COLORS.tick}
                strokeDasharray="3 3"
              />
            ))}
          </g>
        );
      })}
      {layout.rows.map(({ tile: t, lane, dropped, y }) => {
        const tx = laneX[lane];
        const lit = !dropped && (t.number === model.main[0].number || targets.has(t.number));
        const miner = minerLabel(t, layout.labelRoom(t));
        return (
          <g key={t.hash} data-tile={t.number} data-dropped={dropped || undefined}>
            <TileTitle t={t} />
            <rect
              x={tx}
              y={y}
              width={TW}
              height={TH}
              fill={FORK_COLORS.tileFill}
              stroke={tileStroke(dropped, lit)}
              strokeDasharray={dropped ? '3 3' : undefined}
            />
            <text
              x={tx + C.inset}
              y={y + C.numberY}
              fontSize={SVG_TEXT.label}
              fill={FORK_COLORS.ink}
            >
              {tileNumber(t.number)}
            </text>
            <text x={tx + C.inset} y={y + C.hashY} fontSize={SVG_TEXT.tick} fill={FORK_COLORS.tick}>
              {t.hash.slice(2, 2 + TILE_HASH_DIGITS)}
            </text>
            {t.timestamp && (
              <text
                data-time
                x={labelsX}
                y={y + C.numberY}
                fontSize={SVG_TEXT.tick}
                fill={FORK_COLORS.dim}
              >
                {blockClock(t.timestamp)}
              </text>
            )}
            {miner && (
              <text x={labelsX} y={y + C.hashY} fontSize={SVG_TEXT.tick} fill={FORK_COLORS.tick}>
                {miner}
              </text>
            )}
          </g>
        );
      })}
      {model.uncleLinks.map((link, i) => {
        const from = layout.at(link.from);
        if (from.lane !== 1)
          throw new Error(
            `Fork tree: the uncle ${link.from} stands in lane ${from.lane}, not the dropped branch's`
          );
        const to = mainByNumber.get(link.to);
        if (!to)
          throw new Error(`Fork tree: no block ${formatBlockNumber(link.to)} on today's chain`);
        const end = layout.at(to.hash);
        // in the gap between today's lane and the branch's, two arrows 4 px apart
        const gap = (laneX[0] + TW + laneX[1]) / 2 + (i % 2 ? 2 : -2);
        const y0 = from.y + TH / 2;
        const y1 = end.y + TH / 2;
        return (
          <path
            key={`${link.from}-${link.to}`}
            data-uncle-link
            d={`M${laneX[1]} ${y0} C${gap} ${y0} ${gap} ${y1} ${laneX[0] + TW + 1} ${y1}`}
            fill="none"
            stroke={FORK_COLORS.uncleLink}
            strokeWidth="1.2"
            markerEnd={`url(#${marker})`}
          />
        );
      })}
      {model.marks.map((mark, i) => {
        const at = layout.at(mark.hash);
        const before = model.marks.slice(0, i).filter((m) => m.hash === mark.hash).length;
        return (
          <TipMark
            key={mark.n}
            n={mark.n}
            cx={laneX[at.lane] + TW - C.markR - before * C.markStep}
            cy={at.y - C.markLift}
            r={C.markR}
            role="tick"
          />
        );
      })}
    </TreeSvg>
  );
}

export { MARK as TIP_MARKS };
