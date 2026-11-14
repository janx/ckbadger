import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { Utc8Age } from '@/components/ui/utc8-date';
import { StatusClock } from '@/components/layout/status-line';

/**
 * The review's item H: an age that is shown must tick. Utc8Age wrote "20s ago" at render and
 * nothing rendered it again, so a quiet list said "20s ago" for minutes. Every age and the status
 * line's clock read one clock (lib/clock.ts): one timer for the page, however many readers, each
 * reader rendering again only when its own text changes (each second under a minute, each minute
 * under an hour, and so on: the age form's own unit decides). Fake timers and a fake Date stand
 * the clock still between steps, so nothing here reads the wall clock.
 */
const T0 = Date.parse('2026-11-14T12:00:00.000Z');

describe('an age reads the one clock (item H)', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('ticks with no data change: 20s, then 21s a second later, then 1m at 60 s', () => {
    vi.useFakeTimers({ now: T0 });
    render(<Utc8Age timestamp={String(T0 - 20_000)} />);
    expect(screen.getByText('20s ago')).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.getByText('21s ago')).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(38_000));
    expect(screen.getByText('59s ago')).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.getByText('1m ago')).toBeInTheDocument();
  });

  it('runs one timer for a page of 50 ages and the status clock, none after they unmount', () => {
    vi.useFakeTimers({ now: T0 });
    const { container, unmount } = render(
      <>
        <StatusClock />
        <div data-rows="">
          {Array.from({ length: 50 }, (_, i) => (
            <Utc8Age key={i} timestamp={String(T0 - (i + 1) * 1000)} />
          ))}
        </div>
      </>
    );
    const first = () => container.querySelector('[data-rows] time')!.textContent;
    expect(first()).toBe('1s ago');
    expect(vi.getTimerCount()).toBe(1);
    act(() => vi.advanceTimersByTime(5000));
    expect(vi.getTimerCount()).toBe(1);
    expect(first()).toBe('6s ago');
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('keeps ticking where the reader prefers reduced motion: text is not motion', () => {
    vi.useFakeTimers({ now: T0 });
    vi.stubGlobal(
      'matchMedia',
      (query: string) =>
        ({
          matches: query === '(prefers-reduced-motion: reduce)',
          media: query,
          addEventListener: () => {},
          removeEventListener: () => {},
        }) as unknown as MediaQueryList
    );
    render(<Utc8Age timestamp={String(T0 - 20_000)} />);
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.getByText('21s ago')).toBeInTheDocument();
  });
});
