import { useState, useCallback } from 'react';

/** Where a cursor walk stands: the cursor of the page shown and the cursors of the pages before. */
interface Walk {
  cursor: string | undefined;
  history: string[];
  /** The page size the walk was taken at. */
  size: number | undefined;
}

const start = (size: number | undefined): Walk => ({ cursor: undefined, history: [], size });

/**
 * A walk through a cursor-paged list, one page at a time. `pageSize` is the page size the list
 * asks the API for: a change of it (a list that reads 24 or 50 a page from md and 20 below)
 * starts the walk again from the first page, as the cursors taken at the other size name other
 * rows (lane c2-finish, item 1d). A next cursor the walk already stands on is taken once: a quick
 * second click on `next →`, while the page keeps its previous data, asks for the same cursor and
 * must not count a page that was not read (item 1e).
 */
export function useCursorPagination(pageSize?: number) {
  const [walk, setWalk] = useState<Walk>(() => start(pageSize));
  // a change of page size starts the walk again, in the render that sees it (before any read)
  const current = walk.size === pageSize ? walk : start(pageSize);
  if (current !== walk) setWalk(current);

  const goToNext = useCallback((nextCursor: string | null | undefined) => {
    if (!nextCursor) return;
    setWalk((w) =>
      w.cursor === nextCursor
        ? w
        : { ...w, cursor: nextCursor, history: [...w.history, w.cursor ?? ''] }
    );
  }, []);

  const goToPrevious = useCallback(() => {
    setWalk((w) => {
      if (w.history.length === 0) return w;
      const prev = w.history[w.history.length - 1];
      return { ...w, cursor: prev || undefined, history: w.history.slice(0, -1) };
    });
  }, []);

  const reset = useCallback(() => setWalk((w) => start(w.size)), []);

  return {
    cursor: current.cursor,
    page: current.history.length + 1,
    hasPrevious: current.history.length > 0,
    goToNext,
    goToPrevious,
    reset,
  };
}
