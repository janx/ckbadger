import type { ReorgDetail } from '@/lib/api';

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

export const FORK_TREE = {
  /** The desktop board: 96 px tiles on a 112 px pitch, time and miner inside the tile. */
  wide: { pitch: 112, tile: 96, tileH: 58, x0: 30, right: 30, rowGap: 46, top: 46 },
  /**
   * Narrow widths (V4 board): tiles shrink from 96 to no less than 72 px with 20 px between them,
   * and carry only the number and hash; the time and miner hang under the box.
   */
  compact: {
    minTile: 72,
    maxTile: 96,
    tileH: 34,
    x0: 6,
    gap: 20,
    right: 4,
    /** Under a box: two 10 px lines (time, miner) and a 4 px margin for a tip mark. */
    under: 34,
    rowGap: 32,
    top: 40,
  },
} as const;

export interface ForkTreeLayout {
  form: 'wide' | 'compact';
  tileW: number;
  tileH: number;
  /** The canvas width: the tree's own in the wide form, the measured box in the compact one. */
  width: number;
  height: number;
  yMain: number;
  /** Top of each branch row above the main row, row 0 nearest to it. */
  rowY: number[];
  /** Block heights drawn as their own columns, oldest first. */
  columns: number[];
  /** The oldest heights folded into one "+N earlier" column on the left; null when none are. */
  collapsed: { count: number; first: number; last: number } | null;
  /** Whether a tile's time and miner hang under its box (compact) or sit inside it (wide). */
  labelsUnder: boolean;
  /** A height's column x; a folded height gives the "+N earlier" column. */
  x: (number: number) => number;
}

/**
 * Where the fork tree's columns go in a box `width` px wide (V4, `docs/RESPONSIVE_LAYOUT.md`
 * rules 3–4). The desktop tree is kept wherever it fits; narrower, tiles shrink towards 72 px with
 * their time and miner moved under them; and when even 72 px tiles cannot hold every height, the
 * oldest heights fold into one "+N earlier" column instead of the canvas scrolling.
 */
export function forkTreeLayout(model: ForkTreeModel, width: number): ForkTreeLayout {
  if (!Number.isFinite(width) || width <= 0)
    throw new Error(`Fork tree: invalid canvas width ${width}`);
  const heights = model.last - model.first + 1;
  const k = model.rows.length;
  const w = FORK_TREE.wide;
  const wideWidth = w.x0 + (heights - 1) * w.pitch + w.tile + w.right;
  const all = Array.from({ length: heights }, (_, i) => model.first + i);
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
      columns: all,
      collapsed: null,
      labelsUnder: false,
      x: (n) => w.x0 + (n - model.first) * w.pitch,
    };
  }
  const c = FORK_TREE.compact;
  const room = width - c.x0 - c.right;
  const tileFor = (columns: number) => (room - (columns - 1) * c.gap) / columns;
  let shown = heights;
  if (tileFor(shown) < c.minTile) {
    shown = Math.floor((room + c.gap) / (c.minTile + c.gap));
    if (shown < 2)
      throw new Error(
        `Fork tree: a ${width} px canvas cannot hold a "+N earlier" column and one ${c.minTile} px block`
      );
  }
  const tileW = Math.min(c.maxTile, Math.floor(tileFor(shown)));
  const folded = shown < heights ? heights - (shown - 1) : 0;
  const columns = all.slice(folded);
  const pitch = tileW + c.gap;
  const pitchY = c.tileH + c.under + c.rowGap;
  const yMain = c.top + k * pitchY;
  const lead = folded ? 1 : 0;
  return {
    form: 'compact',
    tileW,
    tileH: c.tileH,
    width,
    height: yMain + c.tileH + c.under + 4,
    yMain,
    rowY: model.rows.map((_, i) => yMain - (i + 1) * pitchY),
    columns,
    collapsed: folded
      ? { count: folded, first: model.first, last: model.first + folded - 1 }
      : null,
    labelsUnder: true,
    x: (n) => c.x0 + (n < model.first + folded ? 0 : lead + n - model.first - folded) * pitch,
  };
}
