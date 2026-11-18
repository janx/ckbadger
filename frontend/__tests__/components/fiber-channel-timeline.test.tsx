import { afterEach, describe, expect, it, vi } from 'vitest';
import { stubChartWidth } from '../utils/chart-width';
import { render, screen } from '../utils/test-utils';
import { FiberChannelTimeline } from '@/components/fiber/fiber-channel-timeline';
import fixture from '../fixtures/fiber-channel-c07e.json';
import openChannel from '../fixtures/fiber-open.json';
import forceClosed from '../fixtures/fiber-force-closed.json';
import type { FiberChannelDetail } from '@/lib/api';

describe('FiberChannelTimeline ("On chain, in three steps", FF-C4)', () => {
  it('places opened → force-closed → settled on a time axis with block, date, and the wait', () => {
    const { container } = render(
      <FiberChannelTimeline channel={fixture.channel as FiberChannelDetail} />
    );
    const svg = container.querySelector('svg')!;
    expect(svg).toHaveAttribute(
      'aria-label',
      'Opened, force-closed 8d 3h later, settled after a wait of 6 epochs'
    );
    for (const label of ['opened', 'force-closed', 'settled'])
      expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.getByText('#15,564,812 · 2025-03-06')).toBeInTheDocument();
    expect(screen.getByText('#15,643,185 · 2025-03-15')).toBeInTheDocument();
    expect(screen.getByText('open 8d 3h · payments off chain')).toBeInTheDocument();
    expect(screen.getByText('6 epochs')).toBeInTheDocument();
    const xs = Array.from(svg.querySelectorAll('[data-point]')).map((p) =>
      Number(p.getAttribute('cx'))
    );
    expect(xs).toHaveLength(3);
    expect(xs[0]).toBeLessThan(xs[1]);
    expect(xs[1]).toBeLessThan(xs[2]);
    expect(svg.querySelector('[data-segment="open"]')).toHaveAttribute('stroke', 'var(--c-jade)');
    expect(svg.querySelector('[data-segment="wait"]')).toHaveAttribute('stroke-dasharray', '3 3');
  });

  it('runs an open channel to the indexed tip', () => {
    render(<FiberChannelTimeline channel={openChannel as FiberChannelDetail} />);
    expect(screen.getByText('open 3m 20s so far · payments off chain')).toBeInTheDocument();
    expect(screen.getByText('indexed tip')).toBeInTheDocument();
    expect(screen.getByText('#30 · 2023-11-15')).toBeInTheDocument();
  });

  it('leaves a force close that is not settled waiting at the right edge', () => {
    render(<FiberChannelTimeline channel={forceClosed as FiberChannelDetail} />);
    expect(screen.getByText('force-closed')).toBeInTheDocument();
    expect(screen.getByText('not settled · wait 1 epoch')).toBeInTheDocument();
    expect(screen.queryByText('settled')).toBeNull();
  });

  // Readability review 2026-11-18 (lane A): on a force-closed channel that is not settled,
  // "force-closed" was printed over "not settled · wait 1 epoch" (mainnet, 1280: x 980–1216): both
  // stood above the line's right end. SVG text never wraps, so each label is a box of its own
  // characters at the mono advance (0.6 em of its own font size); no two boxes may meet.
  describe('labels of a force-closed channel stand apart', () => {
    afterEach(() => vi.unstubAllGlobals());
    // the mainnet channel of the review: opened #19,153,812, force-closed #21,025,146
    const mainnet = {
      ...(forceClosed as FiberChannelDetail),
      openBlock: 19153812,
      openTimestamp: '1777000092000',
      closeBlock: 21025146,
      closeTimestamp: '1794795722000',
      observedLifetimeMs: '17795630000',
      lifetimeAnchorBlock: 21025146,
    } satisfies FiberChannelDetail;
    type Box = { text: string; x0: number; x1: number; y0: number; y1: number };
    const boxes = (svg: SVGSVGElement): Box[] =>
      Array.from(svg.querySelectorAll('text')).map((t) => {
        const size = Number(t.getAttribute('font-size'));
        const x = Number(t.getAttribute('x'));
        const y = Number(t.getAttribute('y'));
        const text = t.textContent ?? '';
        const w = text.length * 0.6 * size;
        const anchor = t.getAttribute('text-anchor') ?? 'start';
        const x0 = anchor === 'end' ? x - w : anchor === 'middle' ? x - w / 2 : x;
        // the baseline at y: capitals reach 0.8 em above it, descenders 0.2 em below
        return { text, x0, x1: x0 + w, y0: y - 0.8 * size, y1: y + 0.2 * size };
      });
    const meeting = (all: Box[]) =>
      all.flatMap((a, i) =>
        all
          .slice(i + 1)
          .filter((b) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1)
          .map((b) => `"${a.text}" meets "${b.text}"`)
      );
    // Failed before the fix: at the board width "force-closed" met the wait's label; below 700 px
    // (block and date on lines of their own) the close's date did.
    it.each([null, 608, 358, 326])('at %s px (null: the board width)', (width) => {
      if (width !== null) stubChartWidth(width);
      const { container } = render(<FiberChannelTimeline channel={mainnet} />);
      const svg = container.querySelector('svg')!;
      expect(svg.getAttribute('viewBox')).toMatch(new RegExp(` ${width ?? 1232} \\d+$`));
      expect(screen.getByText('not settled · wait 1 epoch')).toBeInTheDocument();
      expect(meeting(boxes(svg))).toEqual([]);
    });
  });

  describe('at phone width (visual pass, 390)', () => {
    afterEach(() => vi.unstubAllGlobals());
    it('draws 1:1 in its box and splits each point’s block and date onto two lines', () => {
      stubChartWidth(358);
      const { container } = render(
        <FiberChannelTimeline channel={fixture.channel as FiberChannelDetail} />
      );
      expect(container.querySelector('svg')!.getAttribute('viewBox')).toMatch(/^0 -\d+ 358 /);
      expect(screen.getByText('#15,564,812')).toBeInTheDocument();
      expect(screen.getByText('2025-03-06')).toBeInTheDocument();
      expect(screen.queryByText('#15,564,812 · 2025-03-06')).toBeNull();
      // the label above the line sits clear of the segment label; the wait reads beside it
      expect(Number(screen.getByText('settled').getAttribute('y'))).toBeLessThanOrEqual(-30);
      expect(screen.getByText('6 epochs')).toHaveAttribute('text-anchor', 'start');
    });
  });
});
