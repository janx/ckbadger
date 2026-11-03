import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { DEFAULT_PAGE_SIZE, ENTITY_PAGE_SIZE } from '@/lib/pagination';
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
