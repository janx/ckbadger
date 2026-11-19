import { useState, useCallback } from 'react';
import { stampAsked, type Asked, type CursorWalk } from '@/lib/pagination';

/** Where a cursor walk stands: the cursor of the page shown and the cursors of the pages before. */
interface Walk {
  cursor: string | undefined;
  history: string[];
  /** The page size the walk was taken at. */
  size: number | undefined;
}

const start = (size: number | undefined): Walk => ({ cursor: undefined, history: [], size });

/**
 * A walk through a cursor-paged list, one page at a time. It asks for its pages itself: `ask`
 * turns a read of a cursor into a query function whose response is stamped with the page, the
 * cursor and the page size it was asked at (lib/pagination.ts `Asked`), so the page a
 * response was asked for travels with it and no caller pairs the walk's page with another page's
 * response (lane cd-fixes-controls, item 1). It moves on by a response: `goToNext(read)` takes the
 * response's next cursor only where the walk still stands on the page that response was asked for
 * (a quick second click on `next →`, while the page keeps its previous data, hands it the same
 * response again and moves nothing: lane c2-finish, item 1e). `pageSize` is the page size the list
 * asks the API for: a change of it (a list that reads 24 or 50 a page from md and 20 below) starts
 * the walk again from the first page, as the cursors taken at the other size name
 * other rows (item 1d); so does `reset`.
 */
export function useCursorPagination(pageSize?: number): CursorWalk {
  const [walk, setWalk] = useState<Walk>(() => start(pageSize));
  // a change of page size starts the walk again, in the render that sees it (before any read)
  const current = walk.size === pageSize ? walk : start(pageSize);
  if (current !== walk) setWalk(current);
  const page = current.history.length + 1;

  const goToNext = useCallback((read: Asked<{ nextCursor: string | null }>) => {
    setWalk((w) => {
      const at = read.asked;
      const stands =
        !at.kept &&
        at.page === w.history.length + 1 &&
        at.cursor === w.cursor &&
        at.size === w.size;
      if (!stands || !read.nextCursor) return w;
      return { ...w, cursor: read.nextCursor, history: [...w.history, w.cursor ?? ''] };
    });
  }, []);

  const goToPrevious = useCallback(() => {
    setWalk((w) => {
      if (w.history.length === 0) return w;
      const prev = w.history[w.history.length - 1];
      return { ...w, cursor: prev || undefined, history: w.history.slice(0, -1) };
    });
  }, []);

  const reset = useCallback(() => setWalk((w) => start(w.size)), []);

  const at = { page, cursor: current.cursor, size: current.size };
  const ask =
    <R extends object>(fetch: (cursor: string | undefined) => Promise<R>) =>
    async (): Promise<Asked<R>> =>
      stampAsked(await fetch(at.cursor), at);

  return {
    cursor: current.cursor,
    page,
    size: current.size,
    hasPrevious: current.history.length > 0,
    goToNext,
    goToPrevious,
    reset,
    ask,
  };
}
