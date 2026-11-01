import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { PHONE_PAGE_SIZE, usePageSize } from '@/hooks/usePageSize';

function stubViewport(matches: boolean) {
  const listeners = new Set<() => void>();
  const mql = {
    matches,
    addEventListener: (_: string, cb: () => void) => listeners.add(cb),
    removeEventListener: (_: string, cb: () => void) => listeners.delete(cb),
  };
  const matchMedia = vi.fn(() => mql);
  vi.stubGlobal('matchMedia', matchMedia);
  return {
    matchMedia,
    cross(to: boolean) {
      mql.matches = to;
      listeners.forEach((cb) => cb());
    },
  };
}

afterEach(() => vi.unstubAllGlobals());

describe('usePageSize (R2)', () => {
  it('keeps the list’s own size from md up', () => {
    const viewport = stubViewport(true);
    expect(renderHook(() => usePageSize(50)).result.current).toBe(50);
    expect(viewport.matchMedia).toHaveBeenCalledWith('(min-width: 768px)');
  });

  it('reads twenty rows below md, but never more than the list’s own size', () => {
    stubViewport(false);
    expect(PHONE_PAGE_SIZE).toBe(20);
    expect(renderHook(() => usePageSize(50)).result.current).toBe(20);
    expect(renderHook(() => usePageSize(24)).result.current).toBe(20);
    expect(renderHook(() => usePageSize(12)).result.current).toBe(12);
  });

  it('follows the viewport across the breakpoint', () => {
    const viewport = stubViewport(false);
    const { result } = renderHook(() => usePageSize(50));
    expect(result.current).toBe(20);
    act(() => viewport.cross(true));
    expect(result.current).toBe(50);
  });

  it('refuses a page size that is not a positive integer', () => {
    stubViewport(true);
    expect(() => renderHook(() => usePageSize(0))).toThrow('invalid page size 0');
  });
});
