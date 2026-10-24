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

  it('ignores a new array with the same keys', () => {
    const { result, rerender } = renderHook(({ keys }) => useFreshKeys(keys), {
      initialProps: { keys: ['a'] as string[] | null },
    });
    rerender({ keys: ['a'] });
    expect(result.current.size).toBe(0);
  });
});
