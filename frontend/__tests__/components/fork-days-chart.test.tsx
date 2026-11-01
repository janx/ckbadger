import { afterEach, describe, expect, it, vi } from 'vitest';
import { stubChartWidth } from '../utils/chart-width';
import { act, fireEvent, render, screen } from '../utils/test-utils';
import { ForkDaysChart } from '@/components/forks/fork-days-chart';

const days = [
  { date: '2026-10-20', canonicalBlocks: 9000, uncles: 230, nodeSwitches: 0 },
  { date: '2026-10-21', canonicalBlocks: 9100, uncles: 263, nodeSwitches: 12 },
  { date: '2026-10-22', canonicalBlocks: 9050, uncles: 199, nodeSwitches: 3 },
];

describe('ForkDaysChart (grey uncles with jade node switches, FF-L2)', () => {
  it('draws one wide grey bar per day with the jade switches overlaid, not side-by-side needles', () => {
    const { container } = render(<ForkDaysChart days={days} />);
    const grey = container.querySelectorAll('[data-uncles]');
    const jade = container.querySelectorAll('[data-switches]');
    expect(grey).toHaveLength(3);
    expect(jade).toHaveLength(2);
    expect(Number(grey[0].getAttribute('width'))).toBeGreaterThan(300);
    expect(grey[0]).toHaveAttribute('fill', '#a0a8be');
    expect(jade[0]).toHaveAttribute('fill', '#2edba3');
    // jade sits inside its day's grey bar, from the baseline up
    expect(jade[0].getAttribute('x')).toBe(grey[1].getAttribute('x'));
    expect(Number(jade[0].getAttribute('y')) + Number(jade[0].getAttribute('height'))).toBeCloseTo(
      Number(grey[1].getAttribute('y')) + Number(grey[1].getAttribute('height')),
      5
    );
    // a round scale: 100, 200, 300
    expect(screen.getByText('300')).toBeInTheDocument();
  });

  it('reads one day on hover and with the arrow keys', () => {
    const { container } = render(<ForkDaysChart days={days} />);
    fireEvent.mouseEnter(container.querySelectorAll('[data-day]')[1]);
    expect(screen.getByTestId('fork-day-reading')).toHaveTextContent(
      '2026-10-21 · 263 uncles on chain · this node switched branches 12 times'
    );
    const svg = container.querySelector('svg')!;
    fireEvent.keyDown(svg, { key: 'ArrowRight' });
    expect(screen.getByTestId('fork-day-reading')).toHaveTextContent('2026-10-22 · 199 uncles');
  });

  it('a tap pins a day in the line under the chart; a second tap or Escape lets it go (R6)', () => {
    const { container } = render(<ForkDaysChart days={days} />);
    const svg = container.querySelector('svg')!;
    const day = (i: number) => container.querySelectorAll('[data-day]')[i];
    const readout = () => screen.getByRole('status', { name: 'Day readout' });
    expect(svg.compareDocumentPosition(readout()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(readout()).toHaveTextContent('Tap or hover a day, or focus the chart and use ← →.');
    // a tap: the emulated hover, then the click; the pin stays when the pointer leaves
    fireEvent.mouseEnter(day(1));
    fireEvent.click(day(1));
    fireEvent.mouseLeave(svg);
    expect(readout()).toHaveTextContent(
      /^tapped 2026-10-21 · 263 uncles on chain · this node switched branches 12 times/
    );
    expect(day(0)).toHaveAttribute('opacity', '0.55');
    // a second tap on the same day clears it, though the emulated hover is still on it
    fireEvent.mouseEnter(day(1));
    fireEvent.click(day(1));
    expect(readout()).toHaveTextContent('Tap or hover a day');
    fireEvent.click(day(2));
    expect(readout()).toHaveTextContent(/^tapped 2026-10-22 · 199 uncles/);
    act(() => {
      fireEvent.keyDown(window, { key: 'Escape' });
    });
    expect(readout()).toHaveTextContent('Tap or hover a day');
  });

  it('keeps a pinned day across a refetched window, and lets it go once the day is gone (R6)', () => {
    const { container, rerender } = render(<ForkDaysChart days={days} />);
    const readout = () => screen.getByRole('status', { name: 'Day readout' });
    fireEvent.click(container.querySelectorAll('[data-day]')[1]);
    expect(readout()).toHaveTextContent(/^tapped 2026-10-21 · 263 uncles/);
    const next = { date: '2026-10-23', canonicalBlocks: 9000, uncles: 210, nodeSwitches: 0 };
    rerender(<ForkDaysChart days={[...days.slice(1), next]} />);
    expect(readout()).toHaveTextContent(/^tapped 2026-10-21 · 263 uncles/);
    rerender(<ForkDaysChart days={[...days.slice(2), next]} />);
    expect(readout()).toHaveTextContent('Tap or hover a day');
  });

  describe('at phone width (visual pass, 390)', () => {
    afterEach(() => vi.unstubAllGlobals());
    it('draws 1:1 in its box so the axis text stays 11 px', () => {
      stubChartWidth(358);
      const { container } = render(<ForkDaysChart days={days} />);
      const svg = container.querySelector('svg')!;
      expect(svg.getAttribute('viewBox')).toBe('-44 -10 358 154');
      for (const bar of container.querySelectorAll('[data-uncles]'))
        expect(Number(bar.getAttribute('width'))).toBeGreaterThan(50);
    });

    it('ends the x axis on the last day, right-aligned, and drops a tick that would crowd it', () => {
      stubChartWidth(358);
      const month = Array.from({ length: 30 }, (_, i) => ({
        date: `2026-10-${String(i + 1).padStart(2, '0')}`,
        canonicalBlocks: 9000,
        uncles: 200 + i,
        nodeSwitches: 0,
      }));
      render(<ForkDaysChart days={month} />);
      expect(screen.getByText('10-30')).toHaveAttribute('text-anchor', 'end');
      expect(screen.getByText('10-21')).toBeInTheDocument();
      expect(screen.queryByText('10-26')).toBeNull();
    });
  });
});
