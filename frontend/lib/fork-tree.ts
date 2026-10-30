import type { ReorgDetail } from '@/lib/api';

/** One block drawn in the fork tree; time and miner are absent for the fork point stub. */
export interface ForkTreeTile {
  number: number;
  hash: string;
  timestamp: string | null;
  minerLockHash: string | null;
  /** R2b-b: null exactly when `minerLockHash` is null. */
  minerAddress: string | null;
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
  /** ① the tip this node had, ② the tip it switched to. */
  marks: { n: 1 | 2; hash: string }[];
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
        transactionsCount: null,
      });
  }
  main.sort((a, b) => a.number - b.number);
  const all = [...main, ...rows.flatMap((row) => row.tiles)];
  const known = new Set(all.map((tile) => tile.hash));
  const marks = (
    [
      [1, event.oldTipHash],
      [2, event.newTipHash],
    ] as const
  )
    .filter(([, hash]) => known.has(hash))
    .map(([n, hash]) => ({ n, hash }));
  return {
    main,
    rows,
    uncleLinks,
    marks,
    first: Math.min(...all.map((tile) => tile.number)),
    last: Math.max(...all.map((tile) => tile.number)),
  };
}
