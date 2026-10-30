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
      'Opened, force-closed 8 d 3 h later, settled after a wait of 6 epochs'
    );
    for (const label of ['opened', 'force-closed', 'settled'])
      expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.getByText('#15,564,812 · 2025-03-06')).toBeInTheDocument();
    expect(screen.getByText('#15,643,185 · 2025-03-15')).toBeInTheDocument();
    expect(screen.getByText('open 8 d 3 h · payments off chain')).toBeInTheDocument();
    expect(screen.getByText('6 epochs')).toBeInTheDocument();
    const xs = Array.from(svg.querySelectorAll('[data-point]')).map((p) =>
      Number(p.getAttribute('cx'))
    );
    expect(xs).toHaveLength(3);
    expect(xs[0]).toBeLessThan(xs[1]);
    expect(xs[1]).toBeLessThan(xs[2]);
    expect(svg.querySelector('[data-segment="open"]')).toHaveAttribute('stroke', '#2edba3');
    expect(svg.querySelector('[data-segment="wait"]')).toHaveAttribute('stroke-dasharray', '3 3');
  });

  it('runs an open channel to the indexed tip', () => {
    render(<FiberChannelTimeline channel={openChannel as FiberChannelDetail} />);
    expect(screen.getByText('open 3 m 20 s so far · payments off chain')).toBeInTheDocument();
    expect(screen.getByText('indexed tip')).toBeInTheDocument();
    expect(screen.getByText('#30 · 2023-11-15')).toBeInTheDocument();
  });

  it('leaves a force close that is not settled waiting at the right edge', () => {
    render(<FiberChannelTimeline channel={forceClosed as FiberChannelDetail} />);
    expect(screen.getByText('force-closed')).toBeInTheDocument();
    expect(screen.getByText('not settled · wait 1 epoch')).toBeInTheDocument();
    expect(screen.queryByText('settled')).toBeNull();
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
