import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import {
  DEFAULT_PAGE_SIZE,
  ENTITY_PAGE_SIZE,
  mergeAsked,
  rankedPageAt,
  stampAsked,
  timePageAt,
  type Asked,
  type AskedAt,
  type CursorPageRead,
  type CursorWalk,
  type RankedPage,
} from '@/lib/pagination';
import { PHONE_PAGE_SIZE, usePageSize } from '@/hooks/usePageSize';

function stubViewport(matches: boolean) {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches, addEventListener: () => {}, removeEventListener: () => {} }))
  );
}
afterEach(() => vi.unstubAllGlobals());

// Review 2026-11-03, item 2: an entity page's feeds read twenty rows a page so its second tier
// stays within reach; a list page keeps fifty; a phone reads twenty on both.
describe('page sizes', () => {
  it('pages an entity feed by twenty and a list page by fifty', () => {
    expect(ENTITY_PAGE_SIZE).toBe(20);
    expect(DEFAULT_PAGE_SIZE).toBe(50);
    expect(ENTITY_PAGE_SIZE).toBeLessThan(DEFAULT_PAGE_SIZE);
  });

  it('keeps the entity page size on both sides of md', () => {
    stubViewport(true);
    expect(renderHook(() => usePageSize(ENTITY_PAGE_SIZE)).result.current).toBe(20);
    stubViewport(false);
    expect(renderHook(() => usePageSize(ENTITY_PAGE_SIZE)).result.current).toBe(PHONE_PAGE_SIZE);
  });
});

/** Where a page of 20 rows a page was asked: `page`, cursor c{page-1} (none for the first). */
const asked = (page: number, size: number | undefined = 20): AskedAt => ({
  page,
  cursor: page === 1 ? undefined : `c${page - 1}`,
  size,
});

/** A response of `rows` rows at a page size of 20, with or without a count, asked at `page`. */
const read = (
  rows: number,
  more: boolean,
  total?: number | null,
  page = 1
): Asked<CursorPageRead> =>
  stampAsked(
    {
      data: Array.from({ length: rows }, (_, i) => i),
      limit: 20,
      hasMore: more,
      nextCursor: more ? `c${page}` : null,
      total,
    },
    asked(page)
  );

/** A walk standing on `page` (at cursor c{page-1}, or `cursor`): its moves are spies. */
function walk(
  page: number,
  cursor = page === 1 ? undefined : `c${page - 1}`,
  size: number | undefined = 20
): CursorWalk {
  return {
    cursor,
    page,
    size,
    hasPrevious: page > 1,
    goToNext: vi.fn(),
    goToPrevious: vi.fn(),
    reset: vi.fn(),
    ask: vi.fn(),
  };
}

/** The figures of a ranked page, its two ways aside: whether each is there. */
const figures = (at: RankedPage | null) =>
  at && {
    page: at.page,
    pageSize: at.pageSize,
    rows: at.rows,
    count: at.count,
    previous: Boolean(at.onPrevious),
    next: Boolean(at.onNext),
  };

// The controls lane (C2): the pager computes no figure; the figures of a cursor page come from its
// response (the rows, the page size it served, the count) and the page it was asked for, in one
// place. Re-pinned on purpose (lane cd-fixes-controls, item 1): the page is the one the response
// was asked for, which travels with it (`Asked`, stamped by the walk's `ask`), never a number the
// caller pairs with it; and the ways to go are the helper's, from the walk and the response
// (`onPrevious`, `onNext`), where they were the caller's (`hasMore` and the caller's callbacks).
describe('where a page of a cursor-paged list stands', () => {
  it('gives a ranked list’s page its rows, the count and the pages, from the response', () => {
    expect(figures(rankedPageAt('holders', read(20, true, 727), walk(1)))).toEqual({
      page: 1,
      pageSize: 20,
      rows: { from: 1, to: 20 },
      count: { total: 727, pages: 37 },
      previous: false,
      next: true,
    });
    expect(figures(rankedPageAt('holders', read(7, false, 727, 37), walk(37)))).toEqual({
      page: 37,
      pageSize: 20,
      rows: { from: 721, to: 727 },
      count: { total: 727, pages: 37 },
      previous: true,
      next: false,
    });
  });

  it('moves the walk by the response itself: next with the read, back with the walk', () => {
    const w = walk(2);
    const page2 = read(20, true, 727, 2);
    const at = rankedPageAt('holders', page2, w)!;
    at.onNext!();
    expect(w.goToNext).toHaveBeenCalledWith(page2);
    at.onPrevious!();
    expect(w.goToPrevious).toHaveBeenCalled();
  });

  it('reads the page size the response served', () => {
    const small = stampAsked(
      { data: [0], limit: 2, hasMore: false, nextCursor: null, total: 3 },
      { page: 2, cursor: 'c1', size: 2 }
    );
    expect(figures(rankedPageAt('events', small, walk(2)))).toMatchObject({
      rows: { from: 3, to: 3 },
      pageSize: 2,
      count: { total: 3, pages: 2 },
    });
  });

  it('draws nothing for a list on one page, an empty one too', () => {
    expect(rankedPageAt('holders', read(8, false, 8), walk(1))).toBeNull();
    expect(rankedPageAt('holders', read(0, false, 0), walk(1))).toBeNull();
    expect(rankedPageAt('objects', read(8, false), walk(1))).toBeNull();
    expect(timePageAt('transactions', read(3, false, 3), walk(1))).toBeNull();
    expect(timePageAt('transactions', read(0, false, null), walk(1))).toBeNull();
  });

  // Lane c2-finish, item 1a: an ordered list whose response counts no rows is paged by its own
  // "more" flag, its rows and its page; no count, no pages.
  it('pages an ordered list with no count by its rows and its own “more”', () => {
    expect(figures(rankedPageAt('objects', read(20, true, undefined, 2), walk(2)))).toEqual({
      page: 2,
      pageSize: 20,
      rows: { from: 21, to: 40 },
      count: null,
      previous: true,
      next: true,
    });
    expect(figures(rankedPageAt('objects', read(4, false, null, 3), walk(3)))).toMatchObject({
      rows: { from: 41, to: 44 },
      count: null,
      next: false,
    });
  });

  it('gives a list in time its page, and its pages where the response counts the rows', () => {
    const counted = timePageAt('activities', read(20, true, 1000, 3), walk(3))!;
    expect({ page: counted.page, pages: counted.pages }).toEqual({ page: 3, pages: 50 });
    expect(counted.onNewer).toBeDefined();
    expect(counted.onOlder).toBeDefined();
    expect(timePageAt('activities', read(20, true, undefined, 3), walk(3))).toMatchObject({
      page: 3,
    });
    expect(timePageAt('activities', read(20, true, null, 3), walk(3))!.pages).toBeUndefined();
  });
});

// Lane cd-fixes-controls, item 1 (L1 MUST 1): a page number never meets another page's response.
// `next →` moves the walk at once, while the list keeps the previous page's response until the
// next one arrives (keepPreviousData): an address with 25 live cells, 20 a page, read page 1's
// response as page 2 ("rows 21–40 of 25") and the pager threw, the whole page down with it. The
// page a response was asked for travels with it; while the shown read is another page's, the
// pager states that page and offers no next.
describe('a response kept while the next page loads', () => {
  it('states the page whose rows are drawn, and offers no next while the walk is ahead', () => {
    const page1 = read(20, true, 25, 1);
    expect(figures(rankedPageAt('live cells', page1, walk(2)))).toEqual({
      page: 1,
      pageSize: 20,
      rows: { from: 1, to: 20 },
      count: { total: 25, pages: 2 },
      previous: true,
      next: false,
    });
    // the second response arrives
    expect(figures(rankedPageAt('live cells', read(5, false, 25, 2), walk(2)))).toMatchObject({
      page: 2,
      rows: { from: 21, to: 25 },
      count: { total: 25, pages: 2 },
      next: false,
    });
    // a list with no count, and a list in time: the same
    expect(
      figures(rankedPageAt('token kinds', read(20, true, undefined, 1), walk(2)))
    ).toMatchObject({ page: 1, rows: { from: 1, to: 20 }, next: false });
    const time = timePageAt('activities', read(20, true, undefined, 1), walk(2))!;
    expect(time.page).toBe(1);
    expect(time.onOlder).toBeUndefined();
  });

  it('takes a response asked at another page size as kept data: the walk started again at 50', () => {
    // the page size changed at md: the walk stands on page 1 at 50 a page, and the response of
    // page 1 at 20 is shown until the walk's own answers; its next cursor was taken at 20
    expect(
      figures(rankedPageAt('objects', read(20, true, 40, 1), walk(1, undefined, 50)))
    ).toMatchObject({ page: 1, next: false });
  });

  it('takes a response asked at another cursor of the same page as kept data', () => {
    expect(figures(rankedPageAt('objects', read(20, true, 40, 2), walk(2, 'other')))).toMatchObject(
      { page: 2, next: false }
    );
  });

  it('cannot be handed a page number: the page is the response’s own', () => {
    const plain: CursorPageRead = {
      data: [0],
      limit: 20,
      hasMore: true,
      nextCursor: 'c1',
      total: 25,
    };
    // @ts-expect-error a response that no walk asked for is not a page
    expect(() => rankedPageAt('live cells', plain, walk(2))).toThrow();
    // @ts-expect-error neither is a page number beside it
    expect(() => rankedPageAt('live cells', 2, read(20, true, 25))).toThrow();
    // @ts-expect-error nor in time
    expect(() => timePageAt('activities', plain, walk(1))).toThrow();
  });
});

// Lane cd-fixes-controls, item 2 (L1 MUST 2): a ranked list read at two tips is a state, not a
// fault. The position by arithmetic, (page − 1) × size + 1, holds only if the list stood still
// between the reads, which a cursor walk cannot know. Where it and the response's own count cannot
// both hold (rows past the count, a page past the pages), the page is drawn as the list that moved
// under the reader: its page, its ways to go by the response's own "more", no range, no count.
describe('a ranked list read at two tips', () => {
  it('draws a page whose rows run past the count as the list that moved', () => {
    // 727 holders on page 36; one above rank 720 leaves; page 37 at the new tip: 7 rows of 726
    expect(figures(rankedPageAt('holders', read(7, false, 726, 37), walk(37)))).toEqual({
      page: 37,
      pageSize: 20,
      rows: null,
      count: null,
      previous: true,
      next: false,
    });
  });

  it('draws a page past the pages as the list that moved, its next by the response', () => {
    // 721 → 720 across a page boundary: one row after the cursor, page 37 of 36
    expect(figures(rankedPageAt('holders', read(1, false, 720, 37), walk(37)))).toMatchObject({
      page: 37,
      rows: null,
      count: null,
    });
    // more rows still after it: a way on, by the response's own "more"
    expect(figures(rankedPageAt('holders', read(20, true, 700, 37), walk(37)))).toMatchObject({
      rows: null,
      count: null,
      next: true,
    });
    // a list in time: its page, no count of pages
    const time = timePageAt('activities', read(1, false, 720, 37), walk(37))!;
    expect(time.page).toBe(37);
    expect(time.pages).toBeUndefined();
  });

  // Lane c2-finish, item 1b: an empty page past the first is the same state.
  it('gives an empty page past the first its place and no rows, whatever the count says', () => {
    expect(figures(rankedPageAt('holders', read(0, false, 18, 2), walk(2)))).toEqual({
      page: 2,
      pageSize: 20,
      rows: null,
      count: null,
      previous: true,
      next: false,
    });
    expect(figures(rankedPageAt('objects', read(0, false, undefined, 4), walk(4)))).toMatchObject({
      page: 4,
      rows: null,
    });
  });
});

describe('a caller’s or the API’s fault still throws', () => {
  it('refuses a page size or a count that is not one', () => {
    expect(() =>
      rankedPageAt(
        'holders',
        stampAsked({ ...read(20, true, 40), limit: 0 }, asked(1, undefined)),
        walk(1)
      )
    ).toThrow('holders: a page size of 0 rows');
    expect(() => rankedPageAt('holders', read(20, true, -1, 2), walk(2))).toThrow(
      'holders: -1 rows in all'
    );
    expect(() => timePageAt('objects', read(20, true, -1), walk(1))).toThrow(
      'objects: -1 rows in all'
    );
  });

  it('refuses more rows than a page', () => {
    expect(() => rankedPageAt('holders', read(21, true, 40), walk(1))).toThrow(
      'holders: 21 rows on a page of 20'
    );
  });

  it('counts the rows before a page at the size the API served, which it states back', () => {
    // the API clamps a limit to 1–100 and states the size it served: asked 200, served 100 on
    // every page, so page 2's rows are 101–200, counted at 100 (a served size is not a fault)
    const served = stampAsked({ ...read(100, true, 450, 2), limit: 100 }, asked(2, 200));
    expect(figures(rankedPageAt('holders', served, walk(2, 'c1', 200)))).toMatchObject({
      rows: { from: 101, to: 200 },
      pageSize: 100,
      count: { total: 450, pages: 5 },
    });
  });

  it('refuses a first page that holds more rows than its own count says', () => {
    expect(() => rankedPageAt('holders', read(20, true, 12), walk(1))).toThrow(
      'holders: rows 1–20 on the first page of 12'
    );
  });

  // L1 NOTE 8: a response whose next cursor repeats the cursor it was asked with is an API fault,
  // never a `next →` that does nothing; one with more rows and no next cursor is one too.
  it('refuses a next cursor that repeats the one asked, and more with no next cursor', () => {
    const repeating = stampAsked(
      { data: [0, 1], limit: 20, hasMore: true, nextCursor: 'c4', total: undefined },
      asked(5)
    );
    expect(() => rankedPageAt('holders', repeating, walk(5))).toThrow(
      'holders: the next cursor repeats the cursor page 5 was asked with (c4)'
    );
    expect(() => timePageAt('activities', repeating, walk(5))).toThrow(
      'activities: the next cursor repeats the cursor page 5 was asked with (c4)'
    );
    const none = stampAsked(
      { data: [0], limit: 20, hasMore: true, nextCursor: null, total: undefined },
      asked(1)
    );
    expect(() => rankedPageAt('holders', none, walk(1))).toThrow(
      'holders: a page with more rows after it and no next cursor'
    );
  });

  it('refuses an empty first page that says more rows follow', () => {
    expect(() => rankedPageAt('holders', read(0, true, 40), walk(1))).toThrow(
      'holders: an empty first page with more rows after it'
    );
  });
});

// A page merged from several reads (a script's cells across its references) is asked where its
// reads were: where they all were asked at one place, that place; where they were not (one still
// kept from another page), it is kept data, offering no next.
describe('a page merged from several reads', () => {
  it('is asked where all its reads were, kept where they differ, absent with none', () => {
    const a = read(3, true, undefined, 2);
    const b = read(2, false, undefined, 2);
    const merged = mergeAsked([a, b], {
      data: [0, 1, 2, 3, 4],
      limit: 20,
      hasMore: true,
      nextCursor: 'm2',
    });
    expect(figures(rankedPageAt('cells', merged!, walk(2)))).toMatchObject({ page: 2, next: true });
    const mixed = mergeAsked([a, read(2, false, undefined, 1)], {
      data: [0],
      limit: 20,
      hasMore: true,
      nextCursor: 'm2',
    });
    expect(figures(rankedPageAt('cells', mixed!, walk(2)))).toMatchObject({ next: false });
    expect(mergeAsked([undefined], { data: [], limit: 20, hasMore: false, nextCursor: null })).toBe(
      null
    );
  });
});
