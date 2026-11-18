import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { useTightBelowSm } from '@/hooks/useTightBelowSm';

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
    listeners,
    cross(to: boolean) {
      mql.matches = to;
      listeners.forEach((cb) => cb());
    },
  };
}

afterEach(() => vi.unstubAllGlobals());

// R9 (readability 2026-11-18): the tight size only below sm; from sm an id takes the row size.
describe('useTightBelowSm', () => {
  it('asks for Tailwind’s sm, 640 px: the row size from it, the tight size below it', () => {
    const viewport = stubViewport(true);
    expect(renderHook(() => useTightBelowSm()).result.current).toBe('row');
    expect(viewport.matchMedia).toHaveBeenCalledWith('(min-width: 640px)');
    stubViewport(false);
    expect(renderHook(() => useTightBelowSm()).result.current).toBe('tight');
  });

  it('follows the viewport across the breakpoint, and lets go of it on unmount', () => {
    const viewport = stubViewport(false);
    const { result, unmount } = renderHook(() => useTightBelowSm());
    expect(result.current).toBe('tight');
    act(() => viewport.cross(true));
    expect(result.current).toBe('row');
    unmount();
    expect(viewport.listeners.size).toBe(0);
  });
});
