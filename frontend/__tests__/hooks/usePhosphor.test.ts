import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { PHOSPHOR_MS, usePhosphor } from '@/hooks/usePhosphor';

// Material pass v2 (move 4): a live figure flares for 1.2 s after its value changes — never on
// mount, never for a re-render with the same value, and not at all under reduced motion.

function stubReducedMotion(matches: boolean) {
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({
      matches,
      media: '(prefers-reduced-motion: reduce)',
      addEventListener: () => {},
      removeEventListener: () => {},
    }))
  );
}

describe('usePhosphor', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('is unlit on mount and lit for exactly 1.2 s after the value changes', () => {
    const { result, rerender } = renderHook(({ v }) => usePhosphor(v), {
      initialProps: { v: 20911945 },
    });
    expect(result.current).toBeUndefined();
    rerender({ v: 20911946 });
    expect(result.current).toBe('phosphor-flare');
    act(() => vi.advanceTimersByTime(PHOSPHOR_MS - 1));
    expect(result.current).toBe('phosphor-flare');
    act(() => vi.advanceTimersByTime(1));
    expect(result.current).toBeUndefined();
  });

  it('ignores a re-render with the same value, comparing bigints and strings by value', () => {
    const { result, rerender } = renderHook(({ v }) => usePhosphor(v), {
      initialProps: { v: BigInt('8164902351') as string | bigint },
    });
    rerender({ v: BigInt('8164902351') });
    expect(result.current).toBeUndefined();
    rerender({ v: '8.96 s' });
    expect(result.current).toBe('phosphor-flare');
    act(() => vi.advanceTimersByTime(PHOSPHOR_MS));
    rerender({ v: '8.96 s' });
    expect(result.current).toBeUndefined();
  });

  it('does not flare for a figure’s arrival: nothing while loading, then its first value', () => {
    const { result, rerender } = renderHook(({ v }) => usePhosphor(v), {
      initialProps: { v: undefined as number | null | undefined },
    });
    rerender({ v: 20911945 });
    expect(result.current).toBeUndefined();
    rerender({ v: 20911946 });
    expect(result.current).toBe('phosphor-flare');
    act(() => vi.advanceTimersByTime(PHOSPHOR_MS));
    rerender({ v: null });
    expect(result.current).toBeUndefined();
    rerender({ v: 20911947 });
    expect(result.current).toBeUndefined();
  });

  it('stays lit until 1.2 s after the latest of two quick changes', () => {
    const { result, rerender } = renderHook(({ v }) => usePhosphor(v), { initialProps: { v: 1 } });
    rerender({ v: 2 });
    act(() => vi.advanceTimersByTime(800));
    rerender({ v: 3 });
    act(() => vi.advanceTimersByTime(800));
    expect(result.current).toBe('phosphor-flare');
    act(() => vi.advanceTimersByTime(400));
    expect(result.current).toBeUndefined();
  });

  it('is a no-op under reduced motion', () => {
    stubReducedMotion(true);
    const { result, rerender } = renderHook(({ v }) => usePhosphor(v), { initialProps: { v: 1 } });
    rerender({ v: 2 });
    expect(result.current).toBeUndefined();
    act(() => vi.advanceTimersByTime(PHOSPHOR_MS));
    expect(result.current).toBeUndefined();
  });
});
