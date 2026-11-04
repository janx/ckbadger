import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ErrorSign, type SignLines } from '@/components/error-sign';

const ROUTE: SignLines = ['all that is', 'was worked for.', 'this was not.'];
const READINGS: readonly SignLines[] = [
  ROUTE,
  ['nothing is lost,', 'for nothing', 'was ever here.'],
  ['emptiness', 'is capacity,', 'not loss.'],
];

const tubes = () => Array.from(screen.getByRole('heading').querySelectorAll('[data-tube]'));

describe('ErrorSign', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('is the thought as the h1: two tubes that warm up in turn, and a third that never lights', () => {
    render(<ErrorSign lines={ROUTE} />);
    expect(
      screen.getByRole('heading', { level: 1, name: 'all that is was worked for. this was not.' })
    ).toHaveClass('sign');
    const [first, second, dark] = tubes();
    expect(first).toHaveClass('neon', 'on', 'jade');
    expect(second).toHaveClass('neon', 'on', 'aqua');
    expect(dark).toHaveClass('neon', 'off');
    expect(first).not.toHaveClass('lit');
    act(() => vi.advanceTimersByTime(300));
    expect(first).toHaveClass('warm');
    expect(second).not.toHaveClass('warm');
    act(() => vi.advanceTimersByTime(1400));
    expect(first).toHaveClass('lit');
    expect(first).not.toHaveClass('warm');
    expect(second).toHaveClass('warm');
    act(() => vi.advanceTimersByTime(320));
    expect(second).toHaveClass('lit');
    expect(dark).not.toHaveClass('lit');
    expect(dark).not.toHaveClass('warm');
  });

  it('tap a lit line: the sign goes out and warms up again with the next reading of the thought', () => {
    render(<ErrorSign lines={ROUTE} readings={READINGS} />);
    act(() => vi.advanceTimersByTime(3000));
    const [first] = tubes();
    expect(first).toHaveClass('lit');
    expect(first).toHaveAttribute('title', 'another reading');
    fireEvent.click(first);
    expect(
      screen.getByRole('heading', { name: 'nothing is lost, for nothing was ever here.' })
    ).toBeInTheDocument();
    expect(tubes()[0]).not.toHaveClass('lit');
    act(() => vi.advanceTimersByTime(3000));
    expect(tubes()[0]).toHaveClass('lit');
    fireEvent.click(tubes()[0]);
    fireEvent.click(tubes()[0]);
    expect(
      screen.getByRole('heading', { name: 'all that is was worked for. this was not.' })
    ).toBeInTheDocument();
  });

  it('without other readings a lit line is nothing to tap', () => {
    render(<ErrorSign lines={ROUTE} />);
    expect(tubes()[0]).not.toHaveAttribute('title');
    fireEvent.click(tubes()[0]);
    expect(screen.getByRole('heading')).toHaveTextContent('all that is');
  });

  it('hold the dark line two seconds for the inversion; release before it and nothing happens', () => {
    const onInverted = vi.fn();
    render(<ErrorSign lines={ROUTE} onInverted={onInverted} />);
    const dark = tubes()[2];
    fireEvent.pointerDown(dark);
    act(() => vi.advanceTimersByTime(1000));
    fireEvent.pointerUp(dark);
    act(() => vi.advanceTimersByTime(2000));
    expect(onInverted).not.toHaveBeenCalled();
    fireEvent.pointerDown(dark);
    act(() => vi.advanceTimersByTime(2000));
    expect(onInverted).toHaveBeenCalledWith(true);
    act(() => vi.advanceTimersByTime(4000));
    expect(onInverted).toHaveBeenLastCalledWith(false);
    expect(onInverted).toHaveBeenCalledTimes(2);
  });

  it('a block makes the sign flicker 300 ms after the grid lands; the block already known at mount does not', () => {
    const { rerender } = render(<ErrorSign lines={ROUTE} blockKey="0xa" />);
    act(() => vi.advanceTimersByTime(3000));
    expect(screen.getByRole('heading')).not.toHaveClass('sign-flick');
    rerender(<ErrorSign lines={ROUTE} blockKey="0xb" />);
    expect(screen.getByRole('heading')).not.toHaveClass('sign-flick');
    act(() => vi.advanceTimersByTime(300));
    expect(screen.getByRole('heading')).toHaveClass('sign-flick');
  });

  it('an epoch turn lights the dark line once; the error page browns out', () => {
    const { rerender } = render(<ErrorSign lines={ROUTE} blockKey="0xa" brownout />);
    expect(screen.getByRole('heading')).toHaveClass('sign-brownout');
    expect(tubes()[2]).not.toHaveClass('once');
    rerender(<ErrorSign lines={ROUTE} blockKey="0xb" brownout epochTurn />);
    expect(tubes()[2]).toHaveClass('once');
  });

  it('follows the page’s words when they change, starting dark again', () => {
    const { rerender } = render(<ErrorSign lines={ROUTE} />);
    act(() => vi.advanceTimersByTime(3000));
    expect(tubes()[0]).toHaveClass('lit');
    rerender(<ErrorSign lines={['a hash', 'is a fact.', 'a name, a guess.']} />);
    expect(
      screen.getByRole('heading', { name: 'a hash is a fact. a name, a guess.' })
    ).toBeInTheDocument();
    expect(tubes()[0]).not.toHaveClass('lit');
  });

  it('reduced motion: lit at once, no warm-up, no flicker', () => {
    const matchMedia = window.matchMedia;
    window.matchMedia = ((query: string) =>
      ({
        matches: query === '(prefers-reduced-motion: reduce)',
        media: query,
        onchange: null,
        addEventListener: () => {},
        removeEventListener: () => {},
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
      }) as MediaQueryList) as typeof window.matchMedia;
    try {
      const { rerender } = render(<ErrorSign lines={ROUTE} blockKey="0xa" />);
      expect(tubes()[0]).toHaveClass('lit');
      expect(tubes()[1]).toHaveClass('lit');
      expect(tubes()[0]).not.toHaveClass('warm');
      rerender(<ErrorSign lines={ROUTE} blockKey="0xb" />);
      act(() => vi.advanceTimersByTime(300));
      expect(screen.getByRole('heading')).not.toHaveClass('sign-flick');
    } finally {
      window.matchMedia = matchMedia;
    }
  });
});
