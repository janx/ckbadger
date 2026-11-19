import { afterEach, describe, expect, it, vi } from 'vitest';
import { stubChartWidth } from '../utils/chart-width';
import { render, screen } from '../utils/test-utils';
import { FiberChannelTimeline } from '@/components/fiber/fiber-channel-timeline';
import fixture from '../fixtures/fiber-channel-c07e.json';
import openChannel from '../fixtures/fiber-open.json';
import forceClosed from '../fixtures/fiber-force-closed.json';
import type { FiberChannelDetail } from '@/lib/api';
import {
  allTextLines,
  meetings,
  meets,
  pastPx,
  standsInside,
  viewBox,
} from '@/__tests__/utils/svg-text-boxes';

/** A word of the timeline where it should not stand, by what it measures. */
type Fault =
  | { kind: 'past the svg'; text: string; by: number }
  | { kind: 'meets'; a: string; b: string; by: number }
  | { kind: 'meets a mark'; text: string; mark: string };

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
    // Re-pinned on purpose (lane P4, the wait's rule): the wait is said in the settlement's label
    // at the line's right end, as a waiting channel says it ("not settled · wait 1 epoch"); it was
    // "settled" there and "6 epochs" under the dashed segment, a word with no room of its own
    for (const label of ['opened', 'force-closed', 'settled · wait 6 epochs'])
      expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.getByText('#15,564,812 · 2025-03-06')).toBeInTheDocument();
    expect(screen.getByText('#15,643,185 · 2025-03-15')).toBeInTheDocument();
    expect(screen.getByText('open 8d 3h · payments off chain')).toBeInTheDocument();
    expect(screen.queryByText('6 epochs')).toBeNull();
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

  // Lane P4 (PAGE-LANE-NOTES, P4: the wait label ran past the svg on a phone for long-open
  // channels and met "force-closed" on the desktop): the wait has one place, the settlement's label
  // at the line's right end, above the line, as a waiting channel says its wait.
  describe('says the wait in the settlement’s label, at every width', () => {
    afterEach(() => vi.unstubAllGlobals());
    it.each([1232, 608, 358, 326])('at %i px', (width) => {
      stubChartWidth(width);
      const { container } = render(
        <FiberChannelTimeline channel={fixture.channel as FiberChannelDetail} />
      );
      const svg = container.querySelector('svg')!;
      const label = screen.getByText('settled · wait 6 epochs');
      const settle = svg.querySelector('[data-point="settle"]')!;
      // anchored at its end, 6 px right of the settlement, above the line
      expect(label).toHaveAttribute('text-anchor', 'end');
      expect(Number(label.getAttribute('x'))).toBe(Number(settle.getAttribute('cx')) + 6);
      expect(Number(label.getAttribute('y'))).toBeLessThan(Number(settle.getAttribute('cy')));
      // no word of the svg is the wait alone, under the dashed segment
      expect([...svg.querySelectorAll('text')].map((t) => t.textContent)).not.toContain('6 epochs');
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
      // the label above the line sits clear of the segment label. Re-pinned on purpose (lane P4,
      // the wait's rule): the wait reads in it, ending at the settlement (it was "6 epochs" written
      // from 10 px right of the close, which ran past a 326 px svg)
      const settled = screen.getByText('settled · wait 6 epochs');
      expect(Number(settled.getAttribute('y'))).toBeLessThanOrEqual(-30);
      expect(settled).toHaveAttribute('text-anchor', 'end');
      expect(screen.queryByText('6 epochs')).toBeNull();
    });
  });

  // Readability plan 2026-11-18, lane F3 (the figures' text budgets): at the 12 px floor every
  // word of the timeline — a point's name and its block and date, the open segment's words, the
  // wait — stands whole inside the svg, touches no other word, and clears the line and every
  // point's circle, measured by the type modules (0.6 em a glyph; a tight line over its baseline,
  // its descent under it). The real cases: a settled channel, an open one, one force-closed and
  // waiting (mainnet's #19,153,812 → #21,025,146), and one closed together, at the board width
  // (1232), at 640 (608) and on phones (358, 326).
  describe('every word clear of the line, the points and each other (readability F3)', () => {
    afterEach(() => vi.unstubAllGlobals());
    const waiting = {
      ...(forceClosed as FiberChannelDetail),
      openBlock: 19153812,
      openTimestamp: '1777000092000',
      closeBlock: 21025146,
      closeTimestamp: '1794795722000',
      observedLifetimeMs: '17795630000',
      lifetimeAnchorBlock: 21025146,
    } satisfies FiberChannelDetail;
    const together = {
      ...(fixture.channel as FiberChannelDetail),
      state: 'cooperativelyClosed',
      settlementTimestamp: null,
      settlementBlock: null,
    } as FiberChannelDetail;
    /**
     * A channel open for months and settled within days of its close: mainnet's c07e opened at
     * #15,564,812, here force-closed 300 days later and settled 1.5 days after that (its wait is
     * 6 epochs, about a day), so the close and the settlement stand a few pixels apart at the
     * line's right end.
     */
    const DAY_MS = 86_400_000;
    const settledSoon = {
      ...(fixture.channel as FiberChannelDetail),
      closeBlock: 18_464_812,
      closeTimestamp: String(1741228548404 + 300 * DAY_MS),
      observedLifetimeMs: String(300 * DAY_MS),
      lifetimeAnchorBlock: 18_464_812,
      settlementBlock: 18_478_812,
      settlementTimestamp: String(1741228548404 + 301.5 * DAY_MS),
    } as FiberChannelDetail;
    const channels = [
      ['settled', fixture.channel as FiberChannelDetail],
      ['open', openChannel as FiberChannelDetail],
      ['force-closed, waiting', waiting],
      ['closed together', together],
      ['settled within days of its close', settledSoon],
    ] as const;
    const cases = [1232, 608, 358, 326].flatMap((w) =>
      channels.map(([n, c]) => [w, n, c] as const)
    );
    /**
     * The timeline's known faults, each pinned by what it measures (the ruling on review F-B's
     * test finding 1: no case is filtered out of the table). The list only shrinks, and it is
     * empty: lane P4 gave the wait its rule. A settled channel's wait ("6 epochs", 57.6 px at
     * 12 px) was written under the dashed segment, centred between the close and the settlement on
     * the board and from 10 px right of the close below 700 px; where the two points stand a few
     * pixels apart (settled within days of a long life) or the close stands near the right end on
     * a phone, it ran past the svg (by 1.08 px at 326 for the settled channel, by 24.97 / 26.22 /
     * 26.38 px at 608 / 358 / 326 for the one settled within days) or met "force-closed" (by
     * 15.93 px at 1232). The rule: the wait is said in the settlement's own label at the line's
     * right end, "settled · wait 6 epochs", as a waiting channel says it in its label there ("not
     * settled · wait 1 epoch"): 23 glyphs at 12.5 px, 172.5 px, ending 6 px right of the
     * settlement, which stands 40 px in from the svg's right side, so it starts at W − 206.5
     * (119.5 at 326) and stands above the line, where no other word does.
     */
    const KNOWN_FAULTS: readonly {
      width: number;
      channel: string;
      fault: Fault;
      owner: string;
      why: string;
    }[] = [];
    // Re-pinned on purpose (lane P4): 5 → 0, the wait has its rule
    it('pins the known faults it holds, which only shrink: none', () => {
      expect(KNOWN_FAULTS).toHaveLength(0);
      for (const known of KNOWN_FAULTS)
        expect(cases.some(([w, n]) => w === known.width && n === known.channel)).toBe(true);
    });
    it.each(cases)('at %i px, %s', (width, name, channel) => {
      stubChartWidth(width);
      const { container } = render(<FiberChannelTimeline channel={channel} />);
      const svg = container.querySelector('svg')!;
      const lines = allTextLines(svg);
      const faults: Fault[] = [];
      // every word inside the svg, flush allowed
      for (const line of lines)
        if (!standsInside(line, viewBox(svg), 'flush'))
          faults.push({ kind: 'past the svg', text: line.text, by: pastPx(line, viewBox(svg)) });
      // no two words meet
      for (const m of meetings(lines)) faults.push({ kind: 'meets', ...m });
      // no word meets the line's stroke or a point's circle (its stroke too)
      const marks = [
        ...[...svg.querySelectorAll('line[data-segment]')].map((l) => {
          const [x1, x2, y] = ['x1', 'x2', 'y1'].map((a) => Number(l.getAttribute(a)));
          const half = Number(l.getAttribute('stroke-width')) / 2;
          const name = `the ${l.getAttribute('data-segment')} segment`;
          return { name, left: x1, right: x2, top: y - half, bottom: y + half };
        }),
        ...[...svg.querySelectorAll('circle[data-point]')].map((c) => {
          const [cx, cy, r] = ['cx', 'cy', 'r'].map((a) => Number(c.getAttribute(a)));
          const reach = r + Number(c.getAttribute('stroke-width')) / 2;
          const name = `the ${c.getAttribute('data-point')} point`;
          return { name, left: cx - reach, right: cx + reach, top: cy - reach, bottom: cy + reach };
        }),
      ];
      for (const line of lines)
        for (const mark of marks)
          if (meets(line, mark))
            faults.push({ kind: 'meets a mark', text: line.text, mark: mark.name });
      expect(faults, `the timeline at ${width}`).toEqual(
        KNOWN_FAULTS.filter((k) => k.width === width && k.channel === name).map((k) => k.fault)
      );
    });
  });
});
