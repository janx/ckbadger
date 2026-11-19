import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { COPIED_MS, useCopy } from '@/hooks/useCopy';

/**
 * A4 review 3: the one copy path. HexCode swallowed a failure, the transaction page's CopyHash and
 * the name plate's PlateHash had none (an unhandled rejection where `navigator.clipboard` is
 * absent, as on a page served over plain HTTP on a LAN address). One hook: a copy that worked shows
 * its tick for COPIED_MS, one that could not shows nothing and never throws; its timer goes with
 * the component.
 */
const TEXT = `0x${'ab'.repeat(32)}`;

const setClipboard = (clipboard: unknown) => Object.assign(navigator, { clipboard });

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  setClipboard(undefined);
});

describe('useCopy, the one copy path', () => {
  it('shows the tick for COPIED_MS after a copy that worked, then takes it back', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    setClipboard({ writeText });
    const { result } = renderHook(() => useCopy(TEXT));
    let done: boolean | undefined;
    await act(async () => {
      done = await result.current.copy();
    });
    expect(writeText).toHaveBeenCalledWith(TEXT);
    expect(done).toBe(true);
    expect(result.current.copied).toBe(true);
    act(() => vi.advanceTimersByTime(COPIED_MS - 1));
    expect(result.current.copied).toBe(true);
    act(() => vi.advanceTimersByTime(1));
    expect(result.current.copied).toBe(false);
  });

  // Re-pinned (lane c2-finish, rulings round 1): a copy that could not work claims no copy and
  // throws nothing, as before, and now says so for COPIED_MS too (`refused`), on the copied
  // answer's one timer (it set no timer and said nothing: the copy button kept its own word until
  // the next try, which, drawn over the line under it, would cover that line)
  for (const [where, clipboard] of [
    ['is absent', () => undefined],
    ['refuses', () => ({ writeText: vi.fn().mockRejectedValue(new DOMException('denied')) })],
  ] as const)
    it(`claims no copy, throws nothing and says it refused for COPIED_MS where the clipboard ${where}`, async () => {
      setClipboard(clipboard());
      const { result } = renderHook(() => useCopy(TEXT));
      let done: boolean | undefined;
      await act(async () => {
        done = await result.current.copy();
      });
      expect(done).toBe(false);
      expect(result.current.copied).toBe(false);
      expect(result.current.refused).toBe(true);
      expect(vi.getTimerCount()).toBe(1);
      act(() => vi.advanceTimersByTime(COPIED_MS - 1));
      expect(result.current.refused).toBe(true);
      act(() => vi.advanceTimersByTime(1));
      expect(result.current.refused).toBe(false);
      expect(vi.getTimerCount()).toBe(0);
    });

  it('answers a copy that works after one that was refused with copied alone, on one timer', async () => {
    const writeText = vi
      .fn()
      .mockRejectedValueOnce(new DOMException('denied'))
      .mockResolvedValueOnce(undefined);
    setClipboard({ writeText });
    const { result } = renderHook(() => useCopy(TEXT));
    await act(async () => {
      await result.current.copy();
    });
    expect(result.current.refused).toBe(true);
    await act(async () => {
      await result.current.copy();
    });
    expect(result.current.refused).toBe(false);
    expect(result.current.copied).toBe(true);
    expect(vi.getTimerCount()).toBe(1);
  });

  it('leaves no timer behind when it unmounts while the tick shows', async () => {
    setClipboard({ writeText: vi.fn().mockResolvedValue(undefined) });
    const { result, unmount } = renderHook(() => useCopy(TEXT));
    await act(async () => {
      await result.current.copy();
    });
    expect(vi.getTimerCount()).toBe(1);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('starts the tick over on a second copy, with one timer', async () => {
    setClipboard({ writeText: vi.fn().mockResolvedValue(undefined) });
    const { result } = renderHook(() => useCopy(TEXT));
    await act(async () => {
      await result.current.copy();
    });
    act(() => vi.advanceTimersByTime(COPIED_MS - 100));
    await act(async () => {
      await result.current.copy();
    });
    expect(vi.getTimerCount()).toBe(1);
    act(() => vi.advanceTimersByTime(COPIED_MS - 1));
    expect(result.current.copied).toBe(true);
    act(() => vi.advanceTimersByTime(1));
    expect(result.current.copied).toBe(false);
  });
});
