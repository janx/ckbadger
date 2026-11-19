import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { cursorPageFigures, DEFAULT_PAGE_SIZE, ENTITY_PAGE_SIZE, pagesOf } from '@/lib/pagination';
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

// Lane c2-chain: the pager's pages mode computes no figure (components/ui/pager.tsx), so a cursor
// list whose API sent its total hands it the page's figures from that total, the page size it
// asked for, the rows the page holds and the page the walk is on.
describe('cursorPageFigures', () => {
  it('counts the pages of a total, the last one partial (pagesOf, its one path)', () => {
    expect([pagesOf(45, 20), pagesOf(40, 20), pagesOf(1, 20), pagesOf(1682, 20)]).toEqual([
      3, 2, 1, 85,
    ]);
  });

  it('places a full page, the last partial one and a total that fills its pages exactly', () => {
    expect(cursorPageFigures({ page: 1, pageSize: 20, rows: 20, total: 45 })).toEqual({
      from: 1,
      to: 20,
      total: 45,
      pageSize: 20,
      page: 1,
      pages: 3,
    });
    expect(cursorPageFigures({ page: 3, pageSize: 20, rows: 5, total: 45 })).toMatchObject({
      from: 41,
      to: 45,
      pages: 3,
    });
    expect(cursorPageFigures({ page: 2, pageSize: 20, rows: 20, total: 40 })).toMatchObject({
      from: 21,
      to: 40,
      pages: 2,
    });
  });
});
