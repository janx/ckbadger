import { act } from '@testing-library/react';
import { vi } from 'vitest';

/**
 * Lane E4 (every age on screen reads the one clock): a test of an age holds the clock still with
 * fake timers and moves it by hand, so nothing reads the wall clock and an expectation never races
 * a second turning over. `T0` is a whole second: the one clock (lib/clock.ts) ticks on whole
 * seconds, so an age written from `T0 − 20 s` reads 21 s exactly one second of fake time later.
 */
export const T0 = Date.parse('2026-11-14T12:00:00.000Z');

/**
 * Under `vi.useFakeTimers({ now: T0 })`, let a page read its data: fake time moves in 10 ms steps
 * until `ready()` holds, and never a whole second, so the page's first age is the one its fixture
 * sets. A page that needs more fails here, not in a later expectation.
 */
export async function loadUnderStillClock(ready: () => boolean): Promise<void> {
  for (let spent = 0; !ready(); spent += 10) {
    if (spent >= 900) throw new Error('the page did not show its data within 900 ms of fake time');
    await act(() => vi.advanceTimersByTimeAsync(10));
  }
}

/** Move the held clock on by `ms` of fake time and render what its ticks change. */
export function tick(ms: number): void {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}
