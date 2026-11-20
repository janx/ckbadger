import type { ReorgDetail } from '@/lib/api';
import { SVG_TEXT, lineAscentPx } from '@/lib/chart-type';
import { SHORT_FORMS } from '@/lib/shorten';
import { textPx, tightLinePx } from '@/lib/type-metrics';

/** One block drawn in the fork tree; time and miner are absent for the fork point stub. */
export interface ForkTreeTile {
  number: number;
  hash: string;
  timestamp: string | null;
  minerLockHash: string | null;
  /** R2b-b: null exactly when `minerLockHash` is null. */
  minerAddress: string | null;
  /** Round 3 (R-12): the miner's `pools.toml` name; null when none names it. */
  minerPool: string | null;
  transactionsCount: number | null;
}

export interface ForkTreeModel {
  /** Today's chain from the fork point on, plus any later block that took an uncle. */
  main: ForkTreeTile[];
  /** Branches that are not on today's chain, drawn dashed above it. */
  rows: {
    label: 'dropped at this switch' | 'adopted then, dropped since';
    tiles: ForkTreeTile[];
  }[];
  uncleLinks: { from: string; to: number }[];
  /**
   * ① the tip this node had, ② the tip it switched to, ③ the tip a later switch that reversed
   * this one moved it to (`revertedByEvent.newTip`, round 3 R-6b). Only marks whose block is drawn.
   */
  marks: { n: 1 | 2 | 3; hash: string }[];
  first: number;
  last: number;
}

/** The blocks of one fork event laid out as the Fork board's tree (V67), from the API alone. */
export function forkTreeModel(data: ReorgDetail): ForkTreeModel {
  const { event } = data;
  const main: ForkTreeTile[] = [
    {
      number: event.forkPointNumber,
      hash: event.forkPointHash,
      timestamp: null,
      minerLockHash: null,
      minerAddress: null,
      minerPool: null,
      transactionsCount: null,
    },
    ...data.canonicalBranch,
  ];
  const onMain = new Set(main.map((tile) => tile.hash));
  const dropped = data.orphanedBlocks.filter((block) => !onMain.has(block.hash));
  const adoptedDropped = data.adoptedBranch.filter((block) => !onMain.has(block.hash));
  const rows: ForkTreeModel['rows'] = [];
  if (dropped.length) rows.push({ label: 'dropped at this switch', tiles: dropped });
  if (adoptedDropped.length)
    rows.push({ label: 'adopted then, dropped since', tiles: adoptedDropped });
  const uncleLinks = data.orphanedBlocks.flatMap((block) =>
    block.uncleIn && !onMain.has(block.hash)
      ? [{ from: block.hash, to: block.uncleIn.blockNumber }]
      : []
  );
  for (const block of data.orphanedBlocks) {
    const inclusion = block.uncleIn;
    if (!inclusion) continue;
    const there = main.find((tile) => tile.number === inclusion.blockNumber);
    if (there && there.hash !== inclusion.blockHash)
      throw new Error(
        `Fork #${event.forkPointNumber}: uncle inclusion ${inclusion.blockHash} differs from canonical #${inclusion.blockNumber} ${there.hash}`
      );
    if (!there)
      main.push({
        number: inclusion.blockNumber,
        hash: inclusion.blockHash,
        timestamp: null,
        minerLockHash: null,
        minerAddress: null,
        minerPool: null,
        transactionsCount: null,
      });
  }
  main.sort((a, b) => a.number - b.number);
  const all = [...main, ...rows.flatMap((row) => row.tiles)];
  const known = new Set(all.map((tile) => tile.hash));
  const tips: [1 | 2 | 3, string][] = [
    [1, event.oldTipHash],
    [2, event.newTipHash],
  ];
  if (event.revertedByEvent) tips.push([3, event.revertedByEvent.newTip.hash]);
  const marks = tips.filter(([, hash]) => known.has(hash)).map(([n, hash]) => ({ n, hash }));
  return {
    main,
    rows,
    uncleLinks,
    marks,
    first: Math.min(...all.map((tile) => tile.number)),
    last: Math.max(...all.map((tile) => tile.number)),
  };
}

/*
 * The tree's words (readability plan 2026-11-18, lane F3): a tile's number at the label size, its
 * hash, time and miner at the tick size, each a tight line (lib/chart-type.ts), so a tile's height
 * and width follow from them. A miner is named by its address in the id form its room holds (the
 * row form, 13 characters, else the tight form, 11; never cut again) or by its pool's name, cut to
 * the room it is given (U-09).
 */
const LABEL_LINE = tightLinePx(SVG_TEXT.label);
const TICK_LINE = tightLinePx(SVG_TEXT.tick);
/** A miner's address in the row form: `ckb1`, the ellipsis and its last 8 characters. */
const ROW_ADDRESS = SHORT_FORMS.row.address.head + 1 + SHORT_FORMS.row.address.tail;
/** A wide tile's words stand this far in from its sides. */
const WIDE_INSET = 8;
/**
 * A wide tile: its number over its time over its miner, 5 px under its top and 8 px over its foot,
 * where a tip mark on the main row (r 9, 2 px under the tile) reaches 7 px in.
 */
const WIDE_PAD = { top: 5, foot: 8 } as const;
const WIDE_TILE = WIDE_INSET + Math.ceil(textPx(ROW_ADDRESS, SVG_TEXT.tick)) + WIDE_INSET;
/** A column tile: its number over its hash, 3 px clear of its top and foot. */
const COLUMN_PAD = 3;
/** A column tile's words stand this far in from its side. */
const COLUMN_INSET = 6;
/** The hash a tile writes: its first six hex digits. */
export const TILE_HASH_DIGITS = 6;
/** A block's time on the tree: its clock to the millisecond, `09:12:32.761` (12 characters). */
const CLOCK_CHARS = 12;
/** A tip mark in the column hangs whole over its tile: its radius and its centre over the top. */
const COLUMN_MARK_R = 8;

/** The name a branch of the tree goes by, over its lane or its row. */
export const LANE_NAMES = {
  main: 'main chain',
  'dropped at this switch': 'branch',
  'adopted then, dropped since': 'adopted then, dropped since',
} as const;

export const FORK_TREE = {
  /**
   * The desktop board: tiles that hold a miner's 13-character address whole (110 px; 96 when the
   * tick was 10 px) 16 px apart, time and miner inside the tile.
   */
  wide: {
    pitch: WIDE_TILE + 16,
    tile: WIDE_TILE,
    inset: WIDE_INSET,
    /** 5 + 15 + 15 + 15 + 8 = 58 px. */
    tileH: WIDE_PAD.top + LABEL_LINE + 2 * TICK_LINE + WIDE_PAD.foot,
    /** The baselines of the number (and the hash beside it), the time and the miner. */
    numberY: WIDE_PAD.top + lineAscentPx(SVG_TEXT.label),
    timeY: WIDE_PAD.top + LABEL_LINE + lineAscentPx(SVG_TEXT.tick),
    minerY: WIDE_PAD.top + LABEL_LINE + TICK_LINE + lineAscentPx(SVG_TEXT.tick),
    /** A tip mark's radius. */
    markR: 9,
    x0: 30,
    right: 30,
    rowGap: 46,
    top: 46,
  },
  /**
   * The phone form (lane P4, the tree turned to a column, as the pipeline turns on a phone): one
   * block a row, the rows in order of height (at one height today's block first, then the dropped
   * one, then one adopted and dropped since), each branch in a lane of its own — today's chain on
   * the left, the branch that lost beside it, a branch adopted then dropped beside that — and every
   * tile's time and miner beside it, on its row, in one column of words right of the lanes. Nothing
   * folds: the column grows a row a block. Only links run in the lanes and the gaps between them
   * (today's chain down its lane, the branch's dashed line down its own, the uncle arrows in the gap
   * between the first two lanes) and only words stand right of them, so no line crosses a word.
   *
   * A tile holds its number ("#176", 4 characters at the label size, 28.8 px) over the first six
   * digits of its hash (43.2 px at the tick size), 6 px in from its side: 6 + 43.2 + 6 → 56 px. The
   * lanes stand 8 px apart and the words 8 px right of the last lane, so with two lanes the words
   * start at 4 + 56 + 8 + 56 + 8 = 132 px: 192 px of room at a 360 phone (328 − 132 − 4), 222 at a
   * 390 (358); with the third lane at 196 px, 128 px of room at 328. A clock (86.4 px at 12 px) and
   * a miner's address in the row form (93.6 px) stand whole there.
   */
  column: {
    tile: Math.ceil(2 * COLUMN_INSET + textPx(TILE_HASH_DIGITS, SVG_TEXT.tick)),
    /** 3 + 15 + 15 + 3 = 36 px. */
    tileH: COLUMN_PAD + LABEL_LINE + TICK_LINE + COLUMN_PAD,
    /** The baselines of the number and of the hash under it; the time and the miner beside them. */
    numberY: COLUMN_PAD + lineAscentPx(SVG_TEXT.label),
    hashY: COLUMN_PAD + LABEL_LINE + lineAscentPx(SVG_TEXT.tick),
    inset: COLUMN_INSET,
    x0: 4,
    laneGap: 8,
    labelGap: 8,
    right: 4,
    /** A tip mark: r 8, its centre 8 px over its tile's top, so it hangs whole over the tile. */
    markR: COLUMN_MARK_R,
    markLift: COLUMN_MARK_R,
    /** A later mark on a block that already carries one stands this far left of it. */
    markStep: 18,
    /** Rows stand 18 px apart: a mark (16 px and its stroke) hangs between them, clear of both. */
    rowGap: 2 * COLUMN_MARK_R + 2,
    /** The lane names' lines over the lanes, and 18 px under them before the first row. */
    headGap: 2 * COLUMN_MARK_R + 2,
    /** The lines down a lane run this far in from its tiles' left edge, clear of their marks. */
    linkX: 12,
  },
} as const;

/*
 * Checked once, at load: a column tile holds its words, and the marks over it stand clear of the
 * line down its lane.
 */
{
  const c = FORK_TREE.column;
  if (textPx(4, SVG_TEXT.label) + 2 * c.inset > c.tile)
    throw new Error(`fork tree: a ${c.tile} px column tile cannot hold its number`);
  // the first mark's centre stands its radius in from the tile's right edge, a second one
  // `markStep` left of it: 56 − 8 − 18 − 8 = 22 px from the left edge, clear of the line at 12
  if (c.tile - c.markR - c.markStep - c.markR <= c.linkX)
    throw new Error('fork tree: a second tip mark meets the line down its lane');
}

export interface ForkTreeWideLayout {
  form: 'wide';
  tileW: number;
  tileH: number;
  /** The canvas width: the tree's own. */
  width: number;
  height: number;
  yMain: number;
  /** Top of each branch row above the main row, row 0 nearest to it. */
  rowY: number[];
  /** A height's column x. */
  x: (number: number) => number;
  /** The width a tile's time and miner may take: inside the tile, its width less its insets. */
  labelRoom: (tile: ForkTreeTile) => number;
}

/** One row of the column: a block, the lane of its branch (0 today's chain) and its top. */
export interface ForkTreeColumnRow {
  tile: ForkTreeTile;
  lane: number;
  dropped: boolean;
  y: number;
}

export interface ForkTreeColumnLayout {
  form: 'column';
  tileW: number;
  tileH: number;
  /** The canvas width: the measured box. */
  width: number;
  height: number;
  /** Each lane's x: today's chain, then each of the model's rows, in order. */
  laneX: number[];
  /** Each lane's name, on a line of its own over the lanes. */
  laneNames: { text: string; x: number; y: number; anchor: 'start' | 'end' }[];
  /** The rows, top to bottom. */
  rows: ForkTreeColumnRow[];
  /** Where every tile's time and miner start: right of the last lane. */
  labelsX: number;
  /** The row a block stands on, by its hash. */
  at: (hash: string) => ForkTreeColumnRow;
  /** The width a tile's time and miner may take: from `labelsX` to the canvas's right edge. */
  labelRoom: (tile: ForkTreeTile) => number;
}

export type ForkTreeLayout = ForkTreeWideLayout | ForkTreeColumnLayout;

/**
 * Where the fork tree's blocks go in a box `width` px wide (V4, `docs/RESPONSIVE_LAYOUT.md`
 * rules 3–4). The desktop tree is kept wherever it fits; narrower, it turns to a column (lane P4):
 * every block a row, nothing folded, no width scrolled.
 */
export function forkTreeLayout(model: ForkTreeModel, width: number): ForkTreeLayout {
  if (!Number.isFinite(width) || width <= 0)
    throw new Error(`Fork tree: invalid canvas width ${width}`);
  const heights = model.last - model.first + 1;
  const k = model.rows.length;
  const w = FORK_TREE.wide;
  const wideWidth = w.x0 + (heights - 1) * w.pitch + w.tile + w.right;
  if (wideWidth <= width) {
    const yMain = w.top + k * (w.tileH + w.rowGap);
    return {
      form: 'wide',
      tileW: w.tile,
      tileH: w.tileH,
      width: wideWidth,
      height: yMain + w.tileH + 30,
      yMain,
      rowY: model.rows.map((_, i) => yMain - (i + 1) * (w.tileH + w.rowGap)),
      x: (n) => w.x0 + (n - model.first) * w.pitch,
      labelRoom: () => w.tile - 2 * w.inset,
    };
  }
  return columnLayout(model, width);
}

function columnLayout(model: ForkTreeModel, width: number): ForkTreeColumnLayout {
  const c = FORK_TREE.column;
  const lanes = 1 + model.rows.length;
  const laneX = Array.from({ length: lanes }, (_, i) => c.x0 + i * (c.tile + c.laneGap));
  const labelsX = laneX[lanes - 1] + c.tile + c.labelGap;
  const room = width - labelsX - c.right;
  // a clock to the millisecond is the widest word a tile always writes; a miner's tight form is
  // narrower (79.2 px at 12 px), so a room that holds the clock holds a miner
  const clock = textPx(CLOCK_CHARS, SVG_TEXT.tick);
  if (room < clock)
    throw new Error(
      `Fork tree: a ${width} px canvas leaves ${room} px beside ${lanes} lanes, less than a block's time (${clock} px)`
    );
  const names = [LANE_NAMES.main, ...model.rows.map((row) => LANE_NAMES[row.label])].map(
    (text, i) => {
      const x = laneX[i];
      const y = i * LABEL_LINE + lineAscentPx(SVG_TEXT.label);
      // a name starts at its lane, or ends at the canvas's right edge where it would run past it
      return x + textPx(text.length, SVG_TEXT.label) <= width - c.right
        ? { text, x, y, anchor: 'start' as const }
        : { text, x: width - c.right, y, anchor: 'end' as const };
    }
  );
  const order = [
    ...model.main.map((tile) => ({ tile, lane: 0, dropped: false })),
    ...model.rows.flatMap((row, i) =>
      row.tiles.map((tile) => ({ tile, lane: i + 1, dropped: true }))
    ),
  ].sort((a, b) => a.tile.number - b.tile.number || a.lane - b.lane);
  const top = lanes * LABEL_LINE + c.headGap;
  const rows: ForkTreeColumnRow[] = order.map((row, i) => ({
    ...row,
    y: top + i * (c.tileH + c.rowGap),
  }));
  const byHash = new Map(rows.map((row) => [row.tile.hash, row]));
  const at = (hash: string) => {
    const row = byHash.get(hash);
    if (row === undefined) throw new Error(`Fork tree: ${hash} is not a tile of the tree`);
    return row;
  };
  return {
    form: 'column',
    tileW: c.tile,
    tileH: c.tileH,
    width,
    height: top + rows.length * c.tileH + (rows.length - 1) * c.rowGap + c.right,
    laneX,
    laneNames: names,
    rows,
    labelsX,
    at,
    labelRoom: (tile) => {
      at(tile.hash);
      return room;
    },
  };
}
