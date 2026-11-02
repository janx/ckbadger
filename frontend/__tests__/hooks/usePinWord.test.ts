import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { usePinWord } from '@/hooks/usePinWord';

function stubPointer(coarse: boolean) {
  const listeners = new Set<() => void>();
  const mql = {
    matches: coarse,
    addEventListener: (_: string, cb: () => void) => listeners.add(cb),
    removeEventListener: (_: string, cb: () => void) => listeners.delete(cb),
  };
  const matchMedia = vi.fn(() => mql);
  vi.stubGlobal('matchMedia', matchMedia);
  return {
    matchMedia,
    switchTo(to: boolean) {
      mql.matches = to;
      listeners.forEach((cb) => cb());
    },
  };
}

afterEach(() => vi.unstubAllGlobals());

// Consistency round C11: a pinned reading says how it was chosen — "selected" with a mouse or a
// pen, "tapped" only where the pin can only have come from a finger.
describe('usePinWord (C11)', () => {
  it('says "selected" on a pointer device', () => {
    const pointer = stubPointer(false);
    expect(renderHook(() => usePinWord()).result.current).toBe('selected');
    expect(pointer.matchMedia).toHaveBeenCalledWith('(pointer: coarse)');
  });

  it('says "tapped" on a touch screen', () => {
    stubPointer(true);
    expect(renderHook(() => usePinWord()).result.current).toBe('tapped');
  });

  it('follows the primary pointer when it changes', () => {
    const pointer = stubPointer(false);
    const { result } = renderHook(() => usePinWord());
    act(() => pointer.switchTo(true));
    expect(result.current).toBe('tapped');
  });
});
