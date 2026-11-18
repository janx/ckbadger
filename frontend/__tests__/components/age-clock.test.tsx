import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { ClockText, useAge, useClockText, useClockTime, Utc8Age } from '@/components/ui/utc8-date';
import { StatusClock } from '@/components/layout/status-line';
import { TimeInPool } from '@/components/ui/pool-status';
import { formatTimeAgo, formatUtc8Date, formatUtc8DateTime } from '@/lib/utils';
import { CLOCK_BEHIND } from '@/lib/format-age';

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

  // Lane E4: the one path from the clock to the page is `useClockText`; `useAge`, `ClockText` and
  // `useClockTime` are built on it, and the clock's contract holds for each of them.
  it('renders a reader again only when its own text changes', () => {
    vi.useFakeTimers({ now: T0 });
    let renders = 0;
    function Reader() {
      renders++;
      return <span>{useAge(String(T0 - 120_000))}</span>;
    }
    render(<Reader />);
    expect(screen.getByText('2m ago')).toBeInTheDocument();
    expect(renders).toBe(1);
    // 59 ticks of the clock, each one read, none changing "2m ago"
    act(() => vi.advanceTimersByTime(59_000));
    expect(renders).toBe(1);
    act(() => vi.advanceTimersByTime(1000));
    expect(screen.getByText('3m ago')).toBeInTheDocument();
    expect(renders).toBe(2);
  });

  it('writes a clock text in a line as bare text, rendering only the text again', () => {
    vi.useFakeTimers({ now: T0 });
    let lineRenders = 0;
    function Line() {
      lineRenders++;
      return (
        <p>
          updated <ClockText text={(now) => formatTimeAgo(T0 - 20_000, now)} />
        </p>
      );
    }
    const { container } = render(<Line />);
    // no element of its own: the line is laid out as it was
    expect(container.innerHTML).toBe('<p>updated 20s ago</p>');
    act(() => vi.advanceTimersByTime(1000));
    expect(container.innerHTML).toBe('<p>updated 21s ago</p>');
    expect(lineRenders).toBe(1);
  });

  it('gives a reader of several things the clock’s time, rendering it again when what it writes does', () => {
    // 23:59:58 on 2026-11-14 in UTC+8
    const before = Date.parse('2026-11-14T15:59:58.000Z');
    vi.useFakeTimers({ now: before });
    let renders = 0;
    function Today() {
      renders++;
      const now = useClockTime((t) => formatUtc8Date(t));
      return <span data-at={now}>{formatUtc8Date(now)}</span>;
    }
    const { container } = render(<Today />);
    const day = () => container.querySelector('span')!;
    expect(day()).toHaveTextContent('2026-11-14');
    expect(day()).toHaveAttribute('data-at', String(before));
    act(() => vi.advanceTimersByTime(1000));
    expect(renders).toBe(1);
    act(() => vi.advanceTimersByTime(1000));
    expect(renders).toBe(2);
    expect(day()).toHaveTextContent('2026-11-15');
    // the time it gives is the clock's, as of the tick that changed the day
    expect(day()).toHaveAttribute('data-at', String(before + 2000));
  });

  it('runs one timer for a page of readers of every kind, none after they unmount', () => {
    vi.useFakeTimers({ now: T0 });
    function Hover({ i }: { i: number }) {
      return <span title={useClockText((now) => formatTimeAgo(T0 - i * 1000, now))} />;
    }
    function Day() {
      return <span>{formatUtc8Date(useClockTime((t) => formatUtc8Date(t)))}</span>;
    }
    const { unmount } = render(
      <>
        <StatusClock />
        <Day />
        {Array.from({ length: 10 }, (_, i) => (
          <div key={i}>
            <Utc8Age timestamp={String(T0 - i * 1000)} />
            <ClockText text={(now) => formatTimeAgo(T0 - i * 1000, now)} />
            <TimeInPool since={new Date(T0 - i * 1000).toISOString()} />
            <Hover i={i} />
          </div>
        ))}
      </>
    );
    // the status clock and 1 + 10 × 4 = 41 readers of five kinds: one timer
    expect(vi.getTimerCount()).toBe(1);
    act(() => vi.advanceTimersByTime(5000));
    expect(vi.getTimerCount()).toBe(1);
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

// Review fb-logic 3 (the lead's ruling): an age is a figure and stays one unbreakable unit; "this
// device's clock is behind" is words (29 characters, 226 px at 13 px, in Age columns of 94 to 106
// px) and breaks between its words, in a cell or anywhere else. It was one NoBreak with the ages.
describe('an age is one unit, the clock-behind sentence is words', () => {
  afterEach(() => vi.useRealTimers());

  it('holds a real age on one line and lets the sentence break between its words', () => {
    vi.useFakeTimers({ now: T0 });
    const { container } = render(
      <p>
        <Utc8Age timestamp={String(T0 - 20_000)} />
        <Utc8Age timestamp={String(T0 + 60_000)} />
      </p>
    );
    const [age, behind] = Array.from(container.querySelectorAll('time'));
    expect(age).toHaveTextContent('20s ago');
    expect(age.closest('[data-nobreak]')).toHaveClass('whitespace-nowrap');
    expect(behind).toHaveTextContent(CLOCK_BEHIND);
    expect(behind.closest('[data-nobreak], .whitespace-nowrap')).toBeNull();
    // the date-time stays its hover in both
    expect(behind).toHaveAttribute('title', formatUtc8DateTime(String(T0 + 60_000)));
  });
});
