import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { stubChartWidth } from '../utils/chart-width';
import { act, fireEvent, render, screen } from '../utils/test-utils';
import { ForkDaysPlot, ForkDaysReading, useForkDaysPin } from '@/components/forks/fork-days-chart';
import { rects, roundedRects } from '@/__tests__/utils/square-corners';
import { percentOf } from '@/lib/format-unit';
import {
  allTextLines,
  expectApart,
  expectInside,
  textLines,
  viewBox,
} from '@/__tests__/utils/svg-text-boxes';

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
/** A day's rate as the reading writes it: the site's one percentage (`percentOf`). */
const rate = (uncles: number, blocks: number) => percentOf(BigInt(uncles), BigInt(blocks));
// re-pinned (lane P4): the reading names the day's rate and the counts it is computed from (it was
// "2026-10-22 · 199 uncles on chain · this node switched branches 3 times")
const LAST = `2026-10-22 · uncle rate ${rate(199, 9050)} · 199 uncles in 9,050 blocks · this node switched branches 3 times`;

describe('ForkDaysChart (grey uncles with jade node switches, FF-L2)', () => {
  // The hero's column at 1440 (R4: drawn 1:1 at its measured width).
  beforeEach(() => stubChartWidth(1248));
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  // Re-pinned (lane P4; it was "draws one wide grey bar per day with the jade switches overlaid, not
  // side-by-side needles", the uncles' count with the switches inside its bar on one scale): the grey
  // bar is the day's uncle rate; the jade switches stand in their own panel under the rates, in their
  // day's column; the scale is in percent; the one dashed line is the target, a reference (V13).
  it('draws one wide grey rate bar per day, the jade switches in their day’s column of their own panel', () => {
    const { container } = render(<Chart days={days} />);
    const grey = container.querySelectorAll('[data-rate]');
    const jade = container.querySelectorAll('[data-switches]');
    expect(grey).toHaveLength(3);
    expect(jade).toHaveLength(2);
    expect(Number(grey[0].getAttribute('width'))).toBeGreaterThan(300);
    expect(grey[0]).toHaveAttribute('fill', 'var(--c-text)');
    expect(jade[0]).toHaveAttribute('fill', 'var(--c-jade)');
    // jade stands in its day's column, under the rates' baseline
    expect(jade[0].getAttribute('x')).toBe(grey[1].getAttribute('x'));
    expect(Number(jade[0].getAttribute('y'))).toBeGreaterThan(
      Number(grey[1].getAttribute('y')) + Number(grey[1].getAttribute('height'))
    );
    // a round scale in percent: 1 %, 2 %, 3 % (its top in the key's band, unwritten)
    expect(screen.getByText('2%')).toBeInTheDocument();
    // visual finish V13(b): the scale's lines are grid — solid, in the row-rule ink (R5, ruling 4c;
    // re-pinned on purpose: it was the hair); the rates' three and the switches' top
    const grid = [...container.querySelectorAll('svg line[data-role="grid"]')];
    expect(grid).toHaveLength(4);
    for (const line of grid) {
      expect(line).toHaveAttribute('stroke', 'var(--c-row-rule)');
      expect(line).not.toHaveAttribute('stroke-dasharray');
    }
    expect(
      [...container.querySelectorAll('svg line[stroke-dasharray]')].map((l) =>
        l.getAttribute('data-role')
      )
    ).toEqual(['target']);
  });

  // re-pinned (lane P4): five bars as before, the key one swatch (it was two: the switches name
  // their own panel now)
  it('draws its day bars and its key swatch with square ends (U-02, R1)', () => {
    const { container } = render(<Chart days={days} />);
    const svg = container.querySelector('svg')!;
    expect(svg.querySelectorAll('[data-rate], [data-switches]')).toHaveLength(5);
    expect(rects(svg.querySelector('[data-role="chart-name"]')!)).toHaveLength(1);
    expect(roundedRects(svg)).toEqual([]);
  });

  it('is the hero’s picture: at least 280 px tall, its name and key inside the plot, ticks inside in ghost ink (R13)', () => {
    const { container } = render(<Chart days={days} />);
    const svg = container.querySelector('svg')!;
    const [, , w, h] = svg.getAttribute('viewBox')!.split(' ').map(Number);
    expect(w).toBe(1248);
    expect(h).toBeGreaterThanOrEqual(280);
    // the name doubles as the key: a swatch and the words of the series, top left (re-pinned, lane
    // P4: it was the uncles' and the switches' keys; the switches name their own panel)
    const name = svg.querySelector('[data-role="chart-name"]')!;
    expect(name).toHaveTextContent('uncle rate, per day');
    for (const text of Array.from(name.querySelectorAll('text'))) {
      // re-pinned (readability R4): the label role is 12 px (was 11)
      expect(text).toHaveAttribute('font-size', '12');
      expect(Number(text.getAttribute('y'))).toBeLessThanOrEqual(20);
    }
    expect(name.querySelectorAll('rect')).toHaveLength(1);
    // ticks: dim ink, written inside the plot's own gridlines (no axis column); the top value
    // would sit in the name's band, so it is not written (as the DAO page's charts); re-pinned
    // (lane P4): the scale is the rate's, in percent (it was the uncles' '100', '200')
    const ticks = Array.from(svg.querySelectorAll('[data-role="y-tick"]'));
    expect(ticks.map((t) => t.textContent)).toEqual(['1%', '2%']);
    for (const t of ticks) expect(Number(t.getAttribute('y')) - 11).toBeGreaterThanOrEqual(28);
    for (const t of ticks) {
      // R4 (re-pinned on purpose): a tick is a value, in the dim ink, never the ghost
      expect(t).toHaveAttribute('fill', 'var(--c-text-dim)');
      expect(Number(t.getAttribute('x'))).toBe(0);
    }
    // the bars start right of the ticks, and the data clears the name's band
    const grey = Array.from(container.querySelectorAll('[data-rate]'));
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

  // re-pinned (lane P4) here and in the three tests after it: a day's reading opens with its rate
  // and the counts it is computed from (it was "N uncles on chain · this node switched …")
  it('reads one day on hover and with the arrow keys, under the hero section', () => {
    const { container } = render(<Chart days={days} />);
    fireEvent.mouseEnter(container.querySelectorAll('[data-day]')[1]);
    expect(screen.getByTestId('fork-day-reading')).toHaveTextContent(
      '2026-10-21 · uncle rate 2.89% · 263 uncles in 9,100 blocks · this node switched branches 12 times'
    );
    expect(screen.getByTestId('under').contains(screen.getByTestId('fork-day-reading'))).toBe(true);
    const svg = container.querySelector('svg')!;
    fireEvent.keyDown(svg, { key: 'ArrowRight' });
    expect(screen.getByTestId('fork-day-reading')).toHaveTextContent(
      `2026-10-22 · uncle rate ${rate(199, 9050)} · 199 uncles`
    );
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
      /^selected 2026-10-21 · uncle rate 2.89% · 263 uncles in 9,100 blocks · this node switched branches 12 times/
    );
    expect(day(0)).toHaveAttribute('opacity', '0.55');
    // a second tap on the same day clears it, though the emulated hover is still on it
    fireEvent.mouseEnter(day(1));
    fireEvent.click(day(1));
    expect(readout()).toHaveTextContent(new RegExp(`^${LAST}`));
    fireEvent.click(day(2));
    expect(readout()).toHaveTextContent(/^selected 2026-10-22 · uncle rate [0-9.]+% · 199 uncles/);
    act(() => {
      fireEvent.keyDown(window, { key: 'Escape' });
    });
    expect(readout()).toHaveTextContent(new RegExp(`^${LAST}`));
  });

  it('keeps a pinned day across a refetched window, and lets it go once the day is gone (R6)', () => {
    const { container, rerender } = render(<Chart days={days} />);
    const readout = () => screen.getByRole('status', { name: 'Day readout' });
    fireEvent.click(container.querySelectorAll('[data-day]')[1]);
    expect(readout()).toHaveTextContent(/^selected 2026-10-21 · uncle rate 2.89% · 263 uncles/);
    const next = { date: '2026-10-23', canonicalBlocks: 9000, uncles: 210, nodeSwitches: 0 };
    rerender(<Chart days={[...days.slice(1), next]} />);
    expect(readout()).toHaveTextContent(/^selected 2026-10-21 · uncle rate 2.89% · 263 uncles/);
    rerender(<Chart days={[...days.slice(2), next]} />);
    expect(readout()).toHaveTextContent(
      /^2026-10-23 · uncle rate 2.33% · 210 uncles in 9,000 blocks/
    );
  });

  // Readability plan 2026-11-18, P4: "the forks chart draws the rate against its target". The
  // review (forks): the hero's figure is the uncle rate against a 2.50 % target, but the chart drew
  // each day's uncles with no target, and this node's branch switches, keyed as a whole series,
  // were a 2–10 px sliver at the foot of five bars on the uncles' scale.
  describe('the rate against its target, the node’s switches on their own scale (P4)', () => {
    it('draws each day’s uncle rate, uncles ÷ blocks, against a dashed line at the 2.50 % target', () => {
      const { container } = render(<Chart days={days} />);
      const svg = container.querySelector('svg')!;
      const bars = [...svg.querySelectorAll('[data-rate]')];
      expect(bars).toHaveLength(3);
      const height = (b: Element) => Number(b.getAttribute('height'));
      // 230 / 9,000, 263 / 9,100 and 199 / 9,050: the tallest is the second day's
      const rates = days.map((d) => d.uncles / d.canonicalBlocks);
      for (let i = 0; i < 3; i++)
        expect(height(bars[i]) / height(bars[1])).toBeCloseTo(rates[i] / rates[1], 6);
      const target = svg.querySelector('[data-role="target"]')!;
      expect(target).toHaveAttribute('stroke-dasharray');
      const y = Number(target.getAttribute('y1'));
      const foot = (b: Element) => Number(b.getAttribute('y')) + height(b);
      // the target stands at 2.5 / 2.89… of the second bar's height over the baseline
      expect((foot(bars[1]) - y) / height(bars[1])).toBeCloseTo(0.025 / rates[1], 6);
      expect(screen.getByText('target 2.50%')).toBeInTheDocument();
      // the scale is in percent
      expect([...svg.querySelectorAll('[data-role="y-tick"]')].map((t) => t.textContent)).toContain(
        '2%'
      );
    });

    it('draws this node’s branch switches in a panel of their own, under the rates, on their own scale', () => {
      const { container } = render(<Chart days={days} />);
      const svg = container.querySelector('svg')!;
      const rates = [...svg.querySelectorAll('[data-rate]')];
      const switches = [...svg.querySelectorAll('[data-switches]')];
      expect(switches).toHaveLength(2);
      const foot = (b: Element) => Number(b.getAttribute('y')) + Number(b.getAttribute('height'));
      // the panel stands under the rates' baseline
      for (const s of switches)
        expect(Number(s.getAttribute('y'))).toBeGreaterThan(Math.max(...rates.map(foot)));
      // its own scale: 12 and 3 switches, the 12 at least 40 px tall, the 3 a quarter of it
      const [twelve, three] = switches.map((s) => Number(s.getAttribute('height')));
      expect(twelve).toBeGreaterThanOrEqual(40);
      expect(three / twelve).toBeCloseTo(3 / 12, 6);
      expect(svg.querySelector('[data-role="switches-name"]')).toHaveTextContent(
        'this node’s branch switches, per day'
      );
    });

    it('reads a day’s rate with the counts it is computed from', () => {
      const { container } = render(<Chart days={days} />);
      fireEvent.mouseEnter(container.querySelectorAll('[data-day]')[1]);
      // 263 / 9,100 = 2.8901…%, cut to two places
      expect(screen.getByTestId('fork-day-reading')).toHaveTextContent(
        '2026-10-21 · uncle rate 2.89% · 263 uncles in 9,100 blocks · this node switched branches 12 times'
      );
    });
  });

  describe('at phone width (visual pass, 390)', () => {
    it('draws 1:1 in its box so the axis text stays 12 px, the key on two lines', () => {
      stubChartWidth(358);
      const { container } = render(<Chart days={days} />);
      const svg = container.querySelector('svg')!;
      const [x, y, w, h] = svg.getAttribute('viewBox')!.split(' ').map(Number);
      expect([x, y, w]).toEqual([0, 0, 358]);
      expect(h).toBeGreaterThanOrEqual(280);
      for (const bar of container.querySelectorAll('[data-uncles]'))
        expect(Number(bar.getAttribute('width'))).toBeGreaterThan(50);
      // each key's words end inside the column (7.2 px a character at 12 px; re-pinned from 6.6)
      for (const text of Array.from(svg.querySelectorAll('[data-role="chart-name"] text')))
        expect(Number(text.getAttribute('x')) + text.textContent!.length * 7.2).toBeLessThan(358);
    });
  });

  // Readability plan 2026-11-18, lane F3 (the figures' text budgets): at the 12 px floor every
  // word of the plot stands whole in its band (the key in the name band over the plot, a value
  // inside the plot over its own gridline, the first and last day in the axis band under it),
  // touches no other word and no line it is not written on, measured by the type modules (0.6 em
  // a glyph; a tight line over its baseline, its descent under it). The real cases: three days to
  // 263 uncles, and a month to 1,476 a day (a four-character value in its lane), at the hero
  // column at 1280 (1248), at 640 (608) and on a phone (358), where the key takes two lines.
  describe('every word in its band (readability F3)', () => {
    const month = Array.from({ length: 30 }, (_, i) => ({
      date: `2026-10-${String(i + 1).padStart(2, '0')}`,
      canonicalBlocks: 9000,
      uncles: 200 + i * 44,
      nodeSwitches: i % 3,
    }));
    it.each(
      [1248, 608, 358].flatMap((width) => [
        [width, 'three days', days] as const,
        [width, 'a month', month] as const,
      ])
    )('at %i px, %s', (width, _name, list) => {
      stubChartWidth(width);
      const { container } = render(<Chart days={list} />);
      const svg = container.querySelector('svg')!;
      const frame = viewBox(svg);
      // the plot's band: each day's hit area spans it, from the rates' top to the switches' foot;
      // re-pinned (lane P4): the key is one line (the switches name their own panel), the rates
      // end at their bars' foot, and the switches' panel, its name and its one value stand under
      // them
      const hit = container.querySelector('[data-day] rect')!;
      const top = Number(hit.getAttribute('y'));
      const foot = top + Number(hit.getAttribute('height'));
      const bar = container.querySelector('[data-rate]')!;
      const base = Number(bar.getAttribute('y')) + Number(bar.getAttribute('height'));
      const band = (from: number, to: number) => ({ ...frame, top: from, bottom: to });
      const key = allTextLines(svg.querySelector('[data-role="chart-name"]')!);
      expect(key).toHaveLength(1);
      for (const line of key) expectInside(line, band(0, top), 'the key', 'flush');
      for (const tick of [...svg.querySelectorAll('[data-role="y-tick"]')]) {
        const gridline = Number(tick.parentElement!.querySelector('line')!.getAttribute('y1'));
        for (const line of textLines(tick)) {
          expectInside(line, band(top, base), 'a value', 'flush');
          expect(line.bottom, `"${line.text}" stands over its gridline`).toBeLessThan(gridline);
        }
      }
      for (const line of textLines(svg.querySelector('[data-role="target-label"]')!))
        expectInside(line, band(top, base), 'the target', 'flush');
      const name = textLines(svg.querySelector('[data-role="switches-name"]')!);
      const tick = svg.querySelector('[data-role="switches-tick"]')!;
      const panelTop = Number(tick.parentElement!.querySelector('line')!.getAttribute('y1'));
      for (const line of name)
        expectInside(line, band(base, panelTop), 'the panel’s name', 'flush');
      for (const line of textLines(tick)) {
        expectInside(line, band(panelTop, foot), 'the switches’ value', 'flush');
        expect(line.top, `"${line.text}" stands under its gridline`).toBeGreaterThan(panelTop);
      }
      const ends = [...svg.querySelectorAll('[data-day] text')].flatMap(textLines);
      expect(ends).toHaveLength(2);
      for (const line of ends) {
        expectInside(line, band(foot, frame.bottom), 'a day', 'flush');
        expect(line.top, `"${line.text}" clears the axis`).toBeGreaterThan(foot);
      }
      expectApart(allTextLines(svg), 'the plot');
    });
  });
});
