import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { DEFAULT_PAGE_SIZE, ENTITY_PAGE_SIZE, pageCount } from '@/lib/pagination';
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
});
