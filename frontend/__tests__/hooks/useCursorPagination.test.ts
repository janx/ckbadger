import { renderHook, act } from '@testing-library/react';
import { useCursorPagination } from '@/hooks/useCursorPagination';
import type { Asked, CursorWalk } from '@/lib/pagination';

/**
 * A walk through a cursor-paged list. Re-pinned on purpose (lane cd-fixes-controls, item 1): the
 * walk asks for its pages itself (`ask`, which stamps a response with the page, the cursor and the
 * page size it was asked at) and moves on by a response (`goToNext(read)`), where
 * it moved on by a cursor a caller handed it; the behaviours it had are kept, each below.
 */
type Page = { data: number[]; nextCursor: string | null };

/** Ask the walk for its current page: the API answers with `next` as the page's next cursor. */
async function askPage(walk: CursorWalk, next: string | null): Promise<Asked<Page>> {
  return walk.ask(async () => ({ data: [1], nextCursor: next }))();
}

describe('useCursorPagination', () => {
  it('starts on the first page, with no cursor and no history', () => {
    const { result } = renderHook(() => useCursorPagination());
    expect(result.current.cursor).toBeUndefined();
    expect(result.current.page).toBe(1);
    expect(result.current.hasPrevious).toBe(false);
  });

  it('asks the API with its cursor and stamps the response with where it was asked', async () => {
    const { result } = renderHook(() => useCursorPagination(20));
    const asked: (string | undefined)[] = [];
    const first = await result.current.ask(async (cursor) => {
      asked.push(cursor);
      return { data: [1], nextCursor: 'c1' };
    })();
    expect(first.asked).toEqual({ page: 1, cursor: undefined, size: 20 });
    act(() => result.current.goToNext(first));
    const second = await result.current.ask(async (cursor) => {
      asked.push(cursor);
      return { data: [2], nextCursor: null };
    })();
    expect(asked).toEqual([undefined, 'c1']);
    expect(second.asked).toMatchObject({ page: 2, cursor: 'c1' });
  });

  describe('goToNext', () => {
    it('moves to the next cursor of the page it stands on, and keeps the one before', async () => {
      const { result } = renderHook(() => useCursorPagination());
      const first = await askPage(result.current, 'cursor-1');
      act(() => result.current.goToNext(first));
      expect(result.current.cursor).toBe('cursor-1');
      expect(result.current.page).toBe(2);
      expect(result.current.hasPrevious).toBe(true);
    });

    it('stays where a page has no next cursor', async () => {
      const { result } = renderHook(() => useCursorPagination());
      const last = await askPage(result.current, null);
      act(() => result.current.goToNext(last));
      expect(result.current.page).toBe(1);
    });

    // Lane c2-finish, item 1e, kept: a quick double click on `next →` while the page keeps its
    // previous data hands the walk the same response twice; it moves once, so "page 3" never
    // names page 2's rows. Re-pinned: the walk now refuses any response not asked on the page
    // it stands on, which covers the second click.
    it('moves once for a response handed twice, in one act or in two', async () => {
      const { result } = renderHook(() => useCursorPagination());
      const first = await askPage(result.current, 'c1');
      act(() => result.current.goToNext(first));
      act(() => result.current.goToNext(first));
      expect(result.current.page).toBe(2);
      const second = await askPage(result.current, 'c2');
      act(() => {
        result.current.goToNext(second);
        result.current.goToNext(second);
      });
      expect(result.current.page).toBe(3);
      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBe('c1');
      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBeUndefined();
      expect(result.current.page).toBe(1);
    });
  });

  describe('goToPrevious', () => {
    it('walks back through the cursors to the first page, and no further', async () => {
      const { result } = renderHook(() => useCursorPagination());
      act(() => result.current.goToPrevious());
      expect(result.current.page).toBe(1);
      const p1 = await askPage(result.current, 'cursor-1');
      act(() => result.current.goToNext(p1));
      const p2 = await askPage(result.current, 'cursor-2');
      act(() => result.current.goToNext(p2));
      expect(result.current.cursor).toBe('cursor-2');
      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBe('cursor-1');
      expect(result.current.hasPrevious).toBe(true);
      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBeUndefined();
      expect(result.current.hasPrevious).toBe(false);
    });
  });

  describe('reset', () => {
    it('starts the walk again from the first page; a response of a later page moves nothing', async () => {
      const { result } = renderHook(() => useCursorPagination());
      const p1 = await askPage(result.current, 'cursor-1');
      act(() => result.current.goToNext(p1));
      const p2 = await askPage(result.current, 'cursor-2');
      act(() => result.current.reset());
      expect(result.current.cursor).toBeUndefined();
      expect(result.current.hasPrevious).toBe(false);
      // page 2's response, kept while the first page loads again: not the page the walk is on
      act(() => result.current.goToNext(p2));
      expect(result.current.page).toBe(1);
    });
  });

  // Item 1d, kept: a list that asks for another page size below md (24 / 20, 50 / 20) starts again
  // from its first page when the size changes: the cursors of the other size name other rows.
  describe('a change of page size', () => {
    it('starts the list again from its first page', async () => {
      const { result, rerender } = renderHook(({ size }) => useCursorPagination(size), {
        initialProps: { size: 50 },
      });
      const p1 = await askPage(result.current, 'c1');
      act(() => result.current.goToNext(p1));
      const p2 = await askPage(result.current, 'c2');
      act(() => result.current.goToNext(p2));
      expect(result.current.page).toBe(3);
      rerender({ size: 20 });
      expect(result.current.page).toBe(1);
      expect(result.current.cursor).toBeUndefined();
      expect(result.current.hasPrevious).toBe(false);
      expect(result.current.size).toBe(20);
      // the response of page 1 at 50 a page, kept while the walk's own answers, moves nothing:
      // its next cursor was taken at the other size
      const old = await renderHook(() => useCursorPagination(50)).result.current.ask(async () => ({
        data: [1],
        nextCursor: 'old-50',
      }))();
      act(() => result.current.goToNext(old));
      expect(result.current.page).toBe(1);
      // the same size keeps the walk
      const d1 = await askPage(result.current, 'd1');
      act(() => result.current.goToNext(d1));
      rerender({ size: 20 });
      expect(result.current.page).toBe(2);
    });
  });
});
