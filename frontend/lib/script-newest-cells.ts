import type { Cell } from '@/lib/api';

/** Lane S (S1): the script page's "Newest cells" lists this many. */
export const NEWEST_CELLS_COUNT = 5;

export type NewestCellsKind = 'lock' | 'type';

/**
 * Ruling 2: one index per list. A lock reads its lock index, a type its type index; a family
 * registered in both roles reads its lock index — the role its "Its cells" list enumerates first
 * (`script_kind=both` is the lock index, then the type index). A family without a role has none.
 */
export function newestCellsKind(scriptKind: string | null): NewestCellsKind | null {
  if (scriptKind === 'lock' || scriptKind === 'lock+type') return 'lock';
  if (scriptKind === 'type') return 'type';
  return null;
}

const outpoint = (c: Cell) => `${c.txHash}:${c.outputIndex}`;

/**
 * The `cell_by_{lock,type}_code` key order, newest first: block, then tx hash, then output index,
 * all descending. `> 0` when `a` comes after `b`.
 */
function compareNewestFirst(a: Cell, b: Cell): number {
  if (a.createdAtBlock !== b.createdAtBlock) return b.createdAtBlock - a.createdAtBlock;
  if (a.txHash !== b.txHash) return a.txHash < b.txHash ? 1 : -1;
  return b.outputIndex - a.outputIndex;
}

/**
 * The newest `limit` cells of a script, from one `GET /cells/by-script?order=desc` page per
 * registered reference: the pages are merged on the server's own key order (each page is already
 * newest block first; nothing is re-sorted). A page out of that order, a row from the other index,
 * or a cell two references both list is an invariant violation and throws with the reference.
 */
export function mergeNewestFirst(
  kind: NewestCellsKind,
  pages: ReadonlyArray<{ reference: string; cells: readonly Cell[] }>,
  limit: number
): Cell[] {
  const listedBy = new Map<string, string>();
  for (const { reference, cells } of pages) {
    cells.forEach((cell, i) => {
      if (cell.matchedScriptKind !== kind)
        throw new Error(
          `${reference}: cells/by-script script_kind=${kind} returned a ${cell.matchedScriptKind ?? 'unmarked'}-index row ${outpoint(cell)}`
        );
      const other = listedBy.get(outpoint(cell));
      if (other !== undefined)
        throw new Error(`${outpoint(cell)} is listed by both ${other} and ${reference}`);
      listedBy.set(outpoint(cell), reference);
      const previous = cells[i - 1];
      if (previous && compareNewestFirst(previous, cell) >= 0)
        throw new Error(
          `${reference}: cells/by-script order=desc listed ${outpoint(previous)} (#${previous.createdAtBlock}) before ${outpoint(cell)} (#${cell.createdAtBlock}): not newest block first`
        );
    });
  }
  // A k-way merge of the already-ordered pages: take the head that comes first, `limit` times.
  const heads = pages.map(() => 0);
  const merged: Cell[] = [];
  while (merged.length < limit) {
    let best = -1;
    pages.forEach(({ cells }, p) => {
      const candidate = cells[heads[p]];
      if (
        candidate &&
        (best < 0 || compareNewestFirst(candidate, pages[best].cells[heads[best]]) < 0)
      )
        best = p;
    });
    if (best < 0) break;
    merged.push(pages[best].cells[heads[best]]);
    heads[best] += 1;
  }
  return merged;
}
