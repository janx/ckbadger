import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { stubChartWidth } from '../utils/chart-width';
import { act, fireEvent, render, screen } from '../utils/test-utils';
import { ForkDaysPlot, ForkDaysReading, useForkDaysPin } from '@/components/forks/fork-days-chart';

const days = [
  { date: '2026-10-20', canonicalBlocks: 9000, uncles: 230, nodeSwitches: 0 },
  { date: '2026-10-21', canonicalBlocks: 9100, uncles: 263, nodeSwitches: 12 },
  { date: '2026-10-22', canonicalBlocks: 9050, uncles: 199, nodeSwitches: 3 },
];
type Days = typeof days;

/** The page's wiring (hierarchy v3): the plot in the hero section, its reading under it. */
function Chart({ days: list }: { days: Days }) {
  const pin = useForkDaysPin();
  return (
    <>
      <section aria-label="hero">
        <ForkDaysPlot days={list} pin={pin} />
      </section>
      <div data-testid="under">
        <ForkDaysReading days={list} pin={pin} />
      </div>
    </>
  );
}
const LAST = '2026-10-22 · 199 uncles on chain · this node switched branches 3 times';

describe('ForkDaysChart (grey uncles with jade node switches, FF-L2)', () => {
  // The hero's column at 1440 (R4: drawn 1:1 at its measured width).
  beforeEach(() => stubChartWidth(1248));
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it('draws one wide grey bar per day with the jade switches overlaid, not side-by-side needles', () => {
    const { container } = render(<Chart days={days} />);
    const grey = container.querySelectorAll('[data-uncles]');
    const jade = container.querySelectorAll('[data-switches]');
    expect(grey).toHaveLength(3);
    expect(jade).toHaveLength(2);
    expect(Number(grey[0].getAttribute('width'))).toBeGreaterThan(300);
    expect(grey[0]).toHaveAttribute('fill', 'var(--c-text)');
    expect(jade[0]).toHaveAttribute('fill', 'var(--c-jade)');
    // jade sits inside its day's grey bar, from the baseline up
    expect(jade[0].getAttribute('x')).toBe(grey[1].getAttribute('x'));
    expect(Number(jade[0].getAttribute('y')) + Number(jade[0].getAttribute('height'))).toBeCloseTo(
      Number(grey[1].getAttribute('y')) + Number(grey[1].getAttribute('height')),
      5
    );
    // a round scale: 100, 200, 300 (its top in the name's band, unwritten)
    expect(screen.getByText('200')).toBeInTheDocument();
    // visual finish V13(b): the scale's lines are grid — solid, in the row-rule ink #1a1f30
    const grid = [...container.querySelectorAll('svg line[data-role="grid"]')];
    expect(grid).toHaveLength(3);
    for (const line of grid) {
      expect(line).toHaveAttribute('stroke', 'var(--c-base-border-subtle)');
      expect(line).not.toHaveAttribute('stroke-dasharray');
    }
    expect(container.querySelectorAll('svg line[stroke-dasharray]')).toHaveLength(0);
  });

  it('is the hero’s picture: at least 280 px tall, its name and key inside the plot, ticks inside in ghost ink (R13)', () => {
    const { container } = render(<Chart days={days} />);
    const svg = container.querySelector('svg')!;
    const [, , w, h] = svg.getAttribute('viewBox')!.split(' ').map(Number);
    expect(w).toBe(1248);
    expect(h).toBeGreaterThanOrEqual(280);
    // the name doubles as the key: a swatch and the words for each series, top left
    const name = svg.querySelector('[data-role="chart-name"]')!;
    expect(name).toHaveTextContent('uncles on chain, per daythis node’s branch switches');
    for (const text of Array.from(name.querySelectorAll('text'))) {
      expect(text).toHaveAttribute('font-size', '11');
      expect(Number(text.getAttribute('y'))).toBeLessThanOrEqual(20);
    }
    expect(name.querySelectorAll('rect')).toHaveLength(2);
    // ticks: ghost ink, written inside the plot's own gridlines (no axis column); the top value
    // would sit in the name's band, so it is not written (as the DAO page's charts)
    const ticks = Array.from(svg.querySelectorAll('[data-role="y-tick"]'));
    expect(ticks.map((t) => t.textContent)).toEqual(['100', '200']);
    for (const t of ticks) expect(Number(t.getAttribute('y')) - 11).toBeGreaterThanOrEqual(28);
    for (const t of ticks) {
      expect(t).toHaveAttribute('fill', 'var(--c-text-ghost)');
      expect(Number(t.getAttribute('x'))).toBe(0);
    }
    // the bars start right of the ticks, and the data clears the name's band
    const grey = Array.from(container.querySelectorAll('[data-uncles]'));
    expect(Math.min(...grey.map((b) => Number(b.getAttribute('x'))))).toBeGreaterThan(21);
    expect(Math.min(...grey.map((b) => Number(b.getAttribute('y'))))).toBeGreaterThanOrEqual(28);
    // no separate heading and no caption element for the picture
    expect(container.querySelector('h2, figcaption')).toBeNull();
  });

  it('labels the x axis with its first and last day only (R14)', () => {
    const month = Array.from({ length: 30 }, (_, i) => ({
      date: `2026-10-${String(i + 1).padStart(2, '0')}`,
      canonicalBlocks: 9000,
      uncles: 200 + i,
      nodeSwitches: 0,
    }));
    render(<Chart days={month} />);
    expect(screen.getByText('10-01')).toHaveAttribute('text-anchor', 'start');
    expect(screen.getByText('10-30')).toHaveAttribute('text-anchor', 'end');
    for (const d of ['10-06', '10-11', '10-21', '10-26']) expect(screen.queryByText(d)).toBeNull();
  });

  it('reads one day on hover and with the arrow keys, under the hero section', () => {
    const { container } = render(<Chart days={days} />);
    fireEvent.mouseEnter(container.querySelectorAll('[data-day]')[1]);
    expect(screen.getByTestId('fork-day-reading')).toHaveTextContent(
      '2026-10-21 · 263 uncles on chain · this node switched branches 12 times'
    );
    expect(screen.getByTestId('under').contains(screen.getByTestId('fork-day-reading'))).toBe(true);
    const svg = container.querySelector('svg')!;
    fireEvent.keyDown(svg, { key: 'ArrowRight' });
    expect(screen.getByTestId('fork-day-reading')).toHaveTextContent('2026-10-22 · 199 uncles');
    // the keys are named for assistive technology, the sentence for everyone is in tier 2
    expect(svg.getAttribute('aria-label')).toMatch(/← → step through the days/);
  });

  it('reads the latest day while idle, with the section’s one “tap or hover” (R14)', () => {
    render(<Chart days={days} />);
    const readout = screen.getByRole('status', { name: 'Day readout' });
    expect(readout).toHaveTextContent(`${LAST}tap or hover`);
    expect(readout).not.toHaveTextContent(/^selected/);
    const hints = [screen.getByRole('region', { name: 'hero' }), screen.getByTestId('under')].map(
      (el) => (el.textContent!.match(/tap or hover/gi) ?? []).length
    );
    expect(hints).toEqual([0, 1]);
  });

  it('a tap pins a day in the line under the chart; a second tap or Escape lets it go (R6)', () => {
    const { container } = render(<Chart days={days} />);
    const svg = container.querySelector('svg')!;
    const day = (i: number) => container.querySelectorAll('[data-day]')[i];
    const readout = () => screen.getByRole('status', { name: 'Day readout' });
    expect(svg.compareDocumentPosition(readout()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // a tap: the emulated hover, then the click; the pin stays when the pointer leaves
    fireEvent.mouseEnter(day(1));
    fireEvent.click(day(1));
    fireEvent.mouseLeave(svg);
    expect(readout()).toHaveTextContent(
      /^selected 2026-10-21 · 263 uncles on chain · this node switched branches 12 times/
    );
    expect(day(0)).toHaveAttribute('opacity', '0.55');
    // a second tap on the same day clears it, though the emulated hover is still on it
    fireEvent.mouseEnter(day(1));
    fireEvent.click(day(1));
    expect(readout()).toHaveTextContent(new RegExp(`^${LAST}`));
    fireEvent.click(day(2));
    expect(readout()).toHaveTextContent(/^selected 2026-10-22 · 199 uncles/);
    act(() => {
      fireEvent.keyDown(window, { key: 'Escape' });
    });
    expect(readout()).toHaveTextContent(new RegExp(`^${LAST}`));
  });

  it('keeps a pinned day across a refetched window, and lets it go once the day is gone (R6)', () => {
    const { container, rerender } = render(<Chart days={days} />);
    const readout = () => screen.getByRole('status', { name: 'Day readout' });
    fireEvent.click(container.querySelectorAll('[data-day]')[1]);
    expect(readout()).toHaveTextContent(/^selected 2026-10-21 · 263 uncles/);
    const next = { date: '2026-10-23', canonicalBlocks: 9000, uncles: 210, nodeSwitches: 0 };
    rerender(<Chart days={[...days.slice(1), next]} />);
    expect(readout()).toHaveTextContent(/^selected 2026-10-21 · 263 uncles/);
    rerender(<Chart days={[...days.slice(2), next]} />);
    expect(readout()).toHaveTextContent(/^2026-10-23 · 210 uncles on chain/);
  });

  describe('at phone width (visual pass, 390)', () => {
    it('draws 1:1 in its box so the axis text stays 11 px, the key on two lines', () => {
      stubChartWidth(358);
      const { container } = render(<Chart days={days} />);
      const svg = container.querySelector('svg')!;
      const [x, y, w, h] = svg.getAttribute('viewBox')!.split(' ').map(Number);
      expect([x, y, w]).toEqual([0, 0, 358]);
      expect(h).toBeGreaterThanOrEqual(280);
      for (const bar of container.querySelectorAll('[data-uncles]'))
        expect(Number(bar.getAttribute('width'))).toBeGreaterThan(50);
      // each key's words end inside the column (6.6 px a character at 11 px)
      for (const text of Array.from(svg.querySelectorAll('[data-role="chart-name"] text')))
        expect(Number(text.getAttribute('x')) + text.textContent!.length * 6.6).toBeLessThan(358);
    });
  });
});
