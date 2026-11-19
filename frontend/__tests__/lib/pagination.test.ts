import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import {
  DEFAULT_PAGE_SIZE,
  ENTITY_PAGE_SIZE,
  rankedPageAt,
  timePageAt,
  type CursorPageRead,
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

/** A response of `rows` rows at a page size of 20, with or without a count of them all. */
const read = (rows: number, more: boolean, total?: number | null): CursorPageRead => ({
  data: Array.from({ length: rows }, (_, i) => i),
  limit: 20,
  hasMore: more,
  total,
});

// The controls lane (C2): the pager computes no figure; the figures of a cursor page come from its
// response (the rows, the page size it served, the count) and the cursor's page, in one place.
describe('where a page of a cursor-paged list stands', () => {
  // Re-pinned on purpose (lane c2-finish, item 1): a ranked page is the response's rows (or none,
  // past the end), its page size, its "more" flag and, where the response counts the rows, the
  // count and the pages; it was a flat from / to / total / pages that a list with no count or a
  // shrunk list could not give.
  it('gives a ranked list’s page its rows, the count and the pages, from the response', () => {
    expect(rankedPageAt('holders', 1, read(20, true, 727))).toEqual({
      page: 1,
      pageSize: 20,
      hasMore: true,
      rows: { from: 1, to: 20 },
      count: { total: 727, pages: 37 },
    });
    expect(rankedPageAt('holders', 37, read(7, false, 727))).toEqual({
      page: 37,
      pageSize: 20,
      hasMore: false,
      rows: { from: 721, to: 727 },
      count: { total: 727, pages: 37 },
    });
  });

  it('reads the page size the response served, not one of its own', () => {
    expect(rankedPageAt('events', 2, { ...read(1, false, 3), limit: 2 })).toMatchObject({
      rows: { from: 3, to: 3 },
      pageSize: 2,
      count: { total: 3, pages: 2 },
    });
  });

  it('draws nothing for a list on one page, an empty one too', () => {
    expect(rankedPageAt('holders', 1, read(8, false, 8))).toBeNull();
    expect(rankedPageAt('holders', 1, read(0, false, 0))).toBeNull();
    expect(rankedPageAt('objects', 1, read(8, false))).toBeNull();
    expect(timePageAt('transactions', 1, read(3, false, 3))).toBeNull();
    expect(timePageAt('transactions', 1, read(0, false, null))).toBeNull();
  });

  // Item 1a: an ordered list whose response counts no rows (the gallery's search) is paged by its
  // own "more" flag, its rows and its page; no count, no pages. Re-pinned: it was refused.
  it('pages an ordered list with no count by its rows and its own “more”', () => {
    expect(rankedPageAt('objects', 2, read(20, true))).toEqual({
      page: 2,
      pageSize: 20,
      hasMore: true,
      rows: { from: 21, to: 40 },
      count: null,
    });
    expect(rankedPageAt('objects', 3, read(4, false, null))).toEqual({
      page: 3,
      pageSize: 20,
      hasMore: false,
      rows: { from: 41, to: 44 },
      count: null,
    });
  });

  // Item 1b: the list shrank under a reader on page 2 (a re-read on focus): the response is an
  // empty page with a smaller count. It is a state: where it stands and a way back, no rows.
  it('gives an empty page past the first its place and no rows, whatever the count says', () => {
    expect(rankedPageAt('holders', 2, read(0, false, 18))).toEqual({
      page: 2,
      pageSize: 20,
      hasMore: false,
      rows: null,
      count: null,
    });
    expect(rankedPageAt('objects', 4, read(0, false))).toMatchObject({ page: 4, rows: null });
  });

  it('refuses a page size or a count that is not one', () => {
    expect(() => rankedPageAt('holders', 1, { ...read(20, true, 40), limit: 0 })).toThrow(
      'holders: a page size of 0 rows'
    );
    expect(() => rankedPageAt('holders', 2, read(20, true, -1))).toThrow('holders: -1 rows in all');
    expect(() => timePageAt('objects', 1, read(20, true, -1))).toThrow('objects: -1 rows in all');
  });

  it('gives a list in time its page, and its pages where the response counts the rows', () => {
    expect(timePageAt('activities', 3, read(20, true, 1000))).toEqual({ page: 3, pages: 50 });
    expect(timePageAt('activities', 3, read(20, true))).toEqual({ page: 3 });
    expect(timePageAt('activities', 3, read(20, true, null))).toEqual({ page: 3 });
  });
});
