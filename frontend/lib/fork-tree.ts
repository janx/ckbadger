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
/** A compact tile: its number over its hash, 3 px clear of its top and foot. */
const COMPACT_PAD = 3;
/** Under a compact tile: its time over its miner, 4 px under the tile and 4 px over the next row. */
const UNDER_PAD = 4;
const COMPACT_GAP = 20;
/** The least two columns' labels under compact tiles keep between them. */
const LABEL_CLEAR = 4;
/** A compact tile's words, and the "+N earlier" box's, stand this far in from its side. */
const FOLD_INSET = 6;
/** The width a "+N earlier" box needs: its words at the label size and the inset each side. */
const foldTile = (count: number) =>
  Math.ceil(2 * FOLD_INSET + textPx(`+${count} earlier`.length, SVG_TEXT.label));

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
   * Narrow widths (V4 board): tiles shrink from 96 to no less than 72 px with 20 px between them
   * (past 96 only where a fold's "+N earlier" needs it, from "+100 earlier"),
   * and carry only the number and hash, 6 px in from the tile's side; the time and miner hang under
   * the box, in its column: the tile and 16 px of the gap (`labelRun`), so a clock to the
   * millisecond (86.4 px at 12 px) stands whole under a 72 px tile. They start at the tile's left
   * edge and run into the gap after it, except in the last column, where they end at the tile's
   * right edge and run into the gap before it (the ruling on F3-d: the svg keeps 4 px after the
   * last tile, so a fork of four heights draws its four columns at a 390 phone, 6 + 4 × 72 + 3 ×
   * 20 + 4 = 358, its dropped block among them). That gap, before the last column, is the one two
   * columns' labels run into from both sides: where both draw labels in a row, each takes half of
   * it less the clearance, `sharedRun` (8 px: 8 + 4 + 8 = 20), and a miner's form is chosen by
   * that room (review fb-logic 1; `ForkTreeLayout.labelRoom`).
   */
  compact: {
    minTile: 72,
    maxTile: 96,
    /** 3 + 15 + 15 + 3 = 36 px (34 at 10 px). */
    tileH: COMPACT_PAD + LABEL_LINE + TICK_LINE + COMPACT_PAD,
    /** The baselines of the number and of the hash under it. */
    numberY: COMPACT_PAD + lineAscentPx(SVG_TEXT.label),
    hashY: COMPACT_PAD + LABEL_LINE + lineAscentPx(SVG_TEXT.tick),
    markR: 8,
    inset: FOLD_INSET,
    x0: 6,
    gap: COMPACT_GAP,
    labelRun: COMPACT_GAP - LABEL_CLEAR,
    sharedRun: (COMPACT_GAP - LABEL_CLEAR) / 2,
    right: 4,
    /** Under a box: its time and miner lines, 4 px over and under them: 38 px (34 at 10 px). */
    under: UNDER_PAD + 2 * TICK_LINE + UNDER_PAD,
    /** The baselines of the time and the miner, from the tile's foot. */
    timeY: UNDER_PAD + lineAscentPx(SVG_TEXT.tick),
    minerY: UNDER_PAD + TICK_LINE + lineAscentPx(SVG_TEXT.tick),
    rowGap: 32,
    top: 40,
  },
} as const;

/*
 * An unnamed miner is its address in an id form, never cut again (review F-B, logic finding 4): a
 * wide tile holds the row form by its width (WIDE_TILE), and the narrowest room under a compact
 * tile, the tile and its share of a gap two columns' labels run into (72 + 8 = 80 px; review
 * fb-logic 1), holds the tight form (11 characters, 79.2 px at 12 px). Checked once, at load.
 */
const TIGHT_ADDRESS = SHORT_FORMS.tight.address.head + 1 + SHORT_FORMS.tight.address.tail;
if (textPx(TIGHT_ADDRESS, SVG_TEXT.tick) > FORK_TREE.compact.minTile + FORK_TREE.compact.sharedRun)
  throw new Error(
    `fork tree: a ${FORK_TREE.compact.minTile} px tile's narrowest room cannot hold a tight address (${textPx(TIGHT_ADDRESS, SVG_TEXT.tick)} px)`
  );

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
  /**
   * The width a tile's time and miner may take (review fb-logic 1): inside a wide tile, its width
   * less its insets; under a compact one, the tile and its run into a gap, 16 px, or 8 where the
   * gap before the last column is run into from both sides, by this tile's labels and by those of
   * its neighbour in the same row (`FORK_TREE.compact`). A miner's form is chosen by it.
   */
  labelRoom: (tile: ForkTreeTile) => number;
}

/** Whether a tile draws a time or a miner (the fork point's stub and a later uncle's block do not). */
const labelled = (tile: ForkTreeTile | undefined): boolean =>
  tile !== undefined && (tile.timestamp !== null || tile.minerAddress !== null);

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
      labelRoom: () => w.tile - 2 * w.inset,
    };
  }
  const c = FORK_TREE.compact;
  const room = width - c.x0 - c.right;
  const tileFor = (columns: number) => (room - (columns - 1) * c.gap) / columns;
  let shown = heights;
  // The least a tile may be: 72 px, or where the tree folds, its "+N earlier" box's words.
  let least: number = c.minTile;
  if (tileFor(shown) < c.minTile) {
    // Fold the oldest heights into one column. Every column is one tile wide, and the fold's box
    // holds its "+N earlier" (84 px at 12 px for a one-digit count, more than a 72 px tile), so
    // the tiles are at least that wide wherever the tree folds: fewer, wider columns.
    for (;;) {
      shown = Math.floor((room + c.gap) / (least + c.gap));
      if (shown < 2)
        throw new Error(
          `Fork tree: a ${width} px canvas cannot hold a "+N earlier" column and one ${least} px block`
        );
      const need = foldTile(heights - (shown - 1));
      if (need <= least) break;
      least = need;
    }
  }
  // Tiles grow to 96 px where there is room, and never under the least: from "+100 earlier"
  // (99 px) the fold's box is as wide as its words (review F-B, logic finding 8). The columns
  // hold it: `shown` columns of a whole `least` px and their gaps fit the room, so a tile's
  // share, rounded down, is at least `least`.
  const tileW = Math.min(Math.max(c.maxTile, least), Math.floor(tileFor(shown)));
  const folded = shown < heights ? heights - (shown - 1) : 0;
  const columns = all.slice(folded);
  const pitch = tileW + c.gap;
  const pitchY = c.tileH + c.under + c.rowGap;
  const yMain = c.top + k * pitchY;
  const lead = folded ? 1 : 0;
  // Under a compact tile the labels run into the gap after it, and the last column's back into
  // the gap before it: there, where both sides draw labels in one row, each has its share. A
  // folded height draws none under the "+N earlier" box.
  const rooms = new Map<string, number>();
  for (const tiles of [model.main, ...model.rows.map((row) => row.tiles)]) {
    const atHeight = (n: number) =>
      n >= model.first + folded ? tiles.find((tile) => tile.number === n) : undefined;
    const shared = labelled(atHeight(model.last - 1)) && labelled(atHeight(model.last));
    for (const tile of tiles) {
      const edge = tile.number === model.last - 1 || tile.number === model.last;
      rooms.set(tile.hash, tileW + (shared && edge ? c.sharedRun : c.labelRun));
    }
  }
  const labelRoom = (tile: ForkTreeTile) => {
    const room = rooms.get(tile.hash);
    if (room === undefined) throw new Error(`Fork tree: ${tile.hash} is not a tile of the tree`);
    return room;
  };
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
    labelRoom,
  };
}
