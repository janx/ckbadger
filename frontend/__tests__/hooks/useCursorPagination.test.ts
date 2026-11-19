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
    expect(first.asked).toEqual({ page: 1, cursor: undefined, size: 20, list: '[]' });
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

  // Lane cd-fixes-controls, the second read's SHOULD 1: the walk knows the list it pages (a
  // filter, a class, a sort, a search), stamps it on every response and starts again when it
  // changes; it replaces `reset`, which the callers called on a change of the list. A response of
  // the other list, still shown while the new list's first page loads, moves nothing: it was asked
  // of another list (the reviewer's sequence: "all" → "dao", then `older →` before dao arrives).
  describe('a change of the list it pages', () => {
    it('starts the walk again; a response of the other list moves nothing, page one or later', async () => {
      const { result, rerender } = renderHook(
        ({ kind }) => useCursorPagination(20, { list: [kind] }),
        { initialProps: { kind: 'all' } }
      );
      const all1 = await askPage(result.current, 'all-20');
      expect(all1.asked).toEqual({ page: 1, cursor: undefined, size: 20, list: '["all"]' });
      rerender({ kind: 'dao' });
      expect(result.current.list).toBe('["dao"]');
      act(() => result.current.goToNext(all1));
      expect(result.current.page).toBe(1);
      expect(result.current.cursor).toBeUndefined();
      // the same on a later page: the walk on "dao" page 2, back to "all"
      const dao1 = await askPage(result.current, 'dao-20');
      act(() => result.current.goToNext(dao1));
      expect(result.current.page).toBe(2);
      rerender({ kind: 'all' });
      expect(result.current.page).toBe(1);
      expect(result.current.hasPrevious).toBe(false);
    });

    it('keeps its walk while its list stays the same, a new array of the same values included', async () => {
      const { result, rerender } = renderHook(
        ({ kind }) => useCursorPagination(20, { list: [kind, 'newest'] }),
        { initialProps: { kind: 'all' } }
      );
      const p1 = await askPage(result.current, 'c1');
      act(() => result.current.goToNext(p1));
      rerender({ kind: 'all' });
      expect(result.current.page).toBe(2);
    });
  });

  // Item 1d, kept: a list that asks for another page size below md (24 / 20, 50 / 20) starts again
  // from its first page when the size changes: the cursors of the other size name other rows.
  // Lane cd-fixes-controls, at the merge with the asset lane's URL cursor: a walk can start from a
  // cursor (an asset feed opened at the URL's), its first page asked and stamped there, so a
  // response read there is not the newest end's
  describe('a walk that starts from a cursor', () => {
    it('asks its first page there, comes back to it, and starts again when the cursor changes', async () => {
      const { result, rerender } = renderHook(({ from }) => useCursorPagination(20, { from }), {
        initialProps: { from: 'url-7' as string | undefined },
      });
      expect(result.current.cursor).toBe('url-7');
      expect(result.current.hasPrevious).toBe(false);
      const fromUrl = await askPage(result.current, 'url-8');
      expect(fromUrl.asked).toEqual({ page: 1, cursor: 'url-7', size: 20, list: '[]' });
      act(() => result.current.goToNext(fromUrl));
      expect(result.current.cursor).toBe('url-8');
      act(() => result.current.goToPrevious());
      expect(result.current.cursor).toBe('url-7');
      // to the newest end: a new start, and the response read at the URL's cursor moves nothing
      rerender({ from: undefined });
      expect(result.current.cursor).toBeUndefined();
      expect(result.current.page).toBe(1);
      act(() => result.current.goToNext(fromUrl));
      expect(result.current.page).toBe(1);
    });
  });

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
