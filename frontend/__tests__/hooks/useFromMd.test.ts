import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { useFromMd } from '@/hooks/useFromMd';

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

describe('useFromMd', () => {
  it('asks for Tailwind’s md, 768 px, and answers what the viewport matches', () => {
    const viewport = stubViewport(true);
    expect(renderHook(() => useFromMd()).result.current).toBe(true);
    expect(viewport.matchMedia).toHaveBeenCalledWith('(min-width: 768px)');
    stubViewport(false);
    expect(renderHook(() => useFromMd()).result.current).toBe(false);
  });

  it('follows the viewport across the breakpoint, and lets go of it on unmount', () => {
    const viewport = stubViewport(false);
    const { result, unmount } = renderHook(() => useFromMd());
    expect(result.current).toBe(false);
    act(() => viewport.cross(true));
    expect(result.current).toBe(true);
    unmount();
    expect(viewport.listeners.size).toBe(0);
  });
});
