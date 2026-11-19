import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { DEFAULT_PAGE_SIZE, ENTITY_PAGE_SIZE, pageCount, rankedPage } from '@/lib/pagination';
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

// The controls lane (C2, c2-lists): the pager computes no figure (components/ui/pager.tsx); a
// list hands it the figures of the response it draws, from one function.
describe('the figures a pager is handed', () => {
  it('counts the pages a total takes, exactly, at any count', () => {
    expect(pageCount(0, 50)).toBe(0);
    expect(pageCount(1, 50)).toBe(1);
    expect(pageCount(50, 50)).toBe(1);
    expect(pageCount(51, 50)).toBe(2);
    expect(pageCount(20_828_902, 50)).toBe(416_579);
    // the largest count a number holds exactly, 2^53 − 1 rows, 3 a page: 3,002,399,751,580,330
    // whole pages and one row more
    expect(pageCount(Number.MAX_SAFE_INTEGER, 3)).toBe(3_002_399_751_580_331);
    expect(() => pageCount(-1, 50)).toThrow('-1 is not a count of rows');
    expect(() => pageCount(10, 0)).toThrow("0 is not a page's size");
    expect(() => pageCount(1.5, 50)).toThrow('1.5 is not a count of rows');
  });

  it('writes a ranked list’s page from its response: the rows shown, a page’s size, the total', () => {
    const rows = (n: number) => Array.from({ length: n }, (_, i) => i);
    expect(rankedPage('tokens', 1, { data: rows(50), limit: 50, total: 727 })).toEqual({
      from: 1,
      to: 50,
      total: 727,
      pageSize: 50,
      page: 1,
      pages: 15,
    });
    expect(rankedPage('tokens', 15, { data: rows(27), limit: 50, total: 727 })).toEqual({
      from: 701,
      to: 727,
      total: 727,
      pageSize: 50,
      page: 15,
      pages: 15,
    });
    // a page's size is the response's limit: a phone's twenty
    expect(rankedPage('tokens', 3, { data: rows(20), limit: 20, total: 727 })).toMatchObject({
      from: 41,
      to: 60,
      pages: 37,
    });
  });

  it('refuses a page without its total, an empty page and a page that is none', () => {
    expect(() => rankedPage('scripts', 1, { data: [1], limit: 50 })).toThrow(
      "scripts: a ranked list's page came without its total"
    );
    expect(() => rankedPage('scripts', 1, { data: [1], limit: 50, total: null })).toThrow(
      'without its total'
    );
    expect(() => rankedPage('scripts', 2, { data: [], limit: 50, total: 60 })).toThrow(
      'scripts: page 2 holds no row to count from'
    );
    expect(() => rankedPage('scripts', 0, { data: [1], limit: 50, total: 60 })).toThrow(
      'scripts: 0 is not a page of the list'
    );
  });
});
