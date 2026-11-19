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
  it('gives a ranked list’s page its rows, the count and the pages, from the response', () => {
    expect(rankedPageAt('holders', 1, read(20, true, 727))).toEqual({
      from: 1,
      to: 20,
      total: 727,
      pageSize: 20,
      page: 1,
      pages: 37,
    });
    expect(rankedPageAt('holders', 37, read(7, false, 727))).toEqual({
      from: 721,
      to: 727,
      total: 727,
      pageSize: 20,
      page: 37,
      pages: 37,
    });
  });

  it('reads the page size the response served, not one of its own', () => {
    expect(rankedPageAt('events', 2, { ...read(1, false, 3), limit: 2 })).toMatchObject({
      from: 3,
      to: 3,
      pageSize: 2,
      pages: 2,
    });
  });

  it('draws nothing for a list on one page, an empty one too', () => {
    expect(rankedPageAt('holders', 1, read(8, false, 8))).toBeNull();
    expect(rankedPageAt('holders', 1, read(0, false, 0))).toBeNull();
    expect(timePageAt('transactions', 1, read(3, false, 3))).toBeNull();
    expect(timePageAt('transactions', 1, read(0, false, null))).toBeNull();
  });

  it('refuses a ranked page that counts no rows, and a page size or a count that is not one', () => {
    expect(() => rankedPageAt('holders', 2, read(20, true))).toThrow(
      'holders: page 2 of a ranked list states no total'
    );
    expect(() => rankedPageAt('holders', 2, read(20, true, null))).toThrow(/states no total/);
    expect(() => rankedPageAt('holders', 1, { ...read(20, true, 40), limit: 0 })).toThrow(
      'holders: a page size of 0 rows'
    );
    expect(() => timePageAt('objects', 1, read(20, true, -1))).toThrow('objects: -1 rows in all');
  });

  it('gives a list in time its page, and its pages where the response counts the rows', () => {
    expect(timePageAt('activities', 3, read(20, true, 1000))).toEqual({ page: 3, pages: 50 });
    expect(timePageAt('activities', 3, read(20, true))).toEqual({ page: 3 });
    expect(timePageAt('activities', 3, read(20, true, null))).toEqual({ page: 3 });
  });
});
