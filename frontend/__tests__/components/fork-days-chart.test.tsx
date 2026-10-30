import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '../utils/test-utils';
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
});
