import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { useFreshKeys } from '@/hooks/useFreshKeys';

describe('useFreshKeys', () => {
  afterEach(() => vi.useRealTimers());

  it('marks nothing new on the first list, then the keys that appeared, for a while', () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ keys }) => useFreshKeys(keys, 2000), {
      initialProps: { keys: null as string[] | null },
    });
    expect(result.current.size).toBe(0);

    rerender({ keys: ['a', 'b'] });
    expect(result.current.size).toBe(0);

    rerender({ keys: ['c', 'a', 'b'] });
    expect([...result.current]).toEqual(['c']);

    act(() => vi.advanceTimersByTime(2000));
    expect(result.current.size).toBe(0);
  });

  it('still lets go of a fresh key when the list reorders within the hold (no key added)', () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ keys }) => useFreshKeys(keys, 2000), {
      initialProps: { keys: ['a', 'b'] as string[] | null },
    });
    rerender({ keys: ['c', 'a', 'b'] });
    expect([...result.current]).toEqual(['c']);

    // The 10 s poll reorders the socket-merged list 1 s later: nothing new.
    act(() => vi.advanceTimersByTime(1000));
    rerender({ keys: ['a', 'c', 'b'] });
    expect([...result.current]).toEqual(['c']);

    act(() => vi.advanceTimersByTime(1000));
    expect(result.current.size).toBe(0);
  });

  it('holds a newly added key for the full time even when an earlier one is still fresh', () => {
    vi.useFakeTimers();
    const { result, rerender } = renderHook(({ keys }) => useFreshKeys(keys, 2000), {
      initialProps: { keys: ['a'] as string[] | null },
    });
    rerender({ keys: ['b', 'a'] });
    act(() => vi.advanceTimersByTime(1500));
    rerender({ keys: ['c', 'b', 'a'] });
    expect([...result.current]).toEqual(['c']);
    act(() => vi.advanceTimersByTime(1500));
    expect([...result.current]).toEqual(['c']);
    act(() => vi.advanceTimersByTime(500));
    expect(result.current.size).toBe(0);
  });

  it('ignores a new array with the same keys', () => {
    const { result, rerender } = renderHook(({ keys }) => useFreshKeys(keys), {
      initialProps: { keys: ['a'] as string[] | null },
    });
    rerender({ keys: ['a'] });
    expect(result.current.size).toBe(0);
  });
});
