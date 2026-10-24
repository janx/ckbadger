import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import {
  BarChart,
  ChartFigure,
  LineChart,
  SignedBarChart,
  type Bar,
} from '@/components/home/mini-chart';
import { MINUS } from '@/lib/chart-ticks';

const texts = (root: Element, axis: 'x' | 'y') =>
  [...root.querySelectorAll(`[data-axis="${axis}"] text`)].map((t) => ({
    text: t.textContent,
    anchor: t.getAttribute('text-anchor') ?? t.parentElement?.getAttribute('text-anchor'),
    size: t.parentElement?.getAttribute('font-size'),
  }));

describe('mini charts', () => {
  beforeEach(() => stubChartWidth(280));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  const blockTimes = [9.1, 8.6, 8.8, 10.9, 9.57].map((value, i) => ({
    label: `10/${19 + i}`,
    value,
  }));

  it('draws a line with round ticks, first/middle/last date labels anchored inside the plot', () => {
    render(<LineChart points={blockTimes} ariaLabel="Block time" formatValue={(v) => `${v} s`} />);
    const svg = screen.getByRole('img', { name: 'Block time' });
    expect(svg).toHaveAttribute('viewBox', '0 0 280 104');
    expect(texts(svg, 'y').map((t) => t.text)).toEqual(['8', '9', '10', '11']);
    expect(texts(svg, 'x')).toEqual([
      { text: '10/19', anchor: 'start', size: '10' },
      { text: '10/21', anchor: 'middle', size: '10' },
      { text: '10/23', anchor: 'end', size: '10' },
    ]);
    const last = svg.querySelector('[data-role="last"]') as SVGCircleElement;
    expect(last.getAttribute('cx')).toBe('272.0');
    expect(last).toHaveTextContent('10/23: 9.57 s');
  });

  it('puts a reference value on a tick, emphasised, with a dashed line', () => {
    const rates = [2.42, 2.84, 2.64].map((value, i) => ({ label: `d${i}`, value }));
    render(
      <LineChart points={rates} ariaLabel="Uncle rate" reference={2.5} formatValue={String} />
    );
    const svg = screen.getByRole('img', { name: 'Uncle rate' });
    expect(texts(svg, 'y').map((t) => t.text)).toEqual(['2.3', '2.5', '2.7', '2.9']);
    const emphasised = [...svg.querySelectorAll('[data-axis="y"] text')].find(
      (t) => t.getAttribute('fill') === '#a0a8be'
    );
    expect(emphasised).toHaveTextContent('2.5');
    expect(svg.querySelector('[data-role="reference"]')).toHaveAttribute('stroke-dasharray', '4 3');
  });

  it('draws bars from zero, the headline bar solid and the in-progress bar hollow', () => {
    const bars: Bar[] = [
      { label: '09:00', value: 610, state: 'complete' },
      { label: '10:00', value: 731, state: 'highlight' },
      { label: '11:00', value: 326, state: 'inProgress' },
    ];
    render(<BarChart bars={bars} ariaLabel="Transactions" formatValue={String} />);
    const svg = screen.getByRole('img', { name: 'Transactions' });
    expect(texts(svg, 'y').map((t) => t.text)).toEqual(['0', '500', '1k']);
    expect(svg.querySelector('[data-role="base"]')).toHaveAttribute('stroke', '#343c50');

    const solid = svg.querySelectorAll('rect[data-bar="complete"], rect[data-bar="highlight"]');
    for (const r of solid) {
      expect(Number(r.getAttribute('y')) + Number(r.getAttribute('height'))).toBeCloseTo(84, 5);
    }
    expect(svg.querySelector('[data-bar="highlight"]')).toHaveAttribute('fill-opacity', '1');
    expect(svg.querySelector('[data-bar="complete"]')).toHaveAttribute('fill-opacity', '0.45');
    const hollow = svg.querySelector('[data-bar="inProgress"]') as SVGRectElement;
    expect(hollow).toHaveAttribute('stroke-dasharray', '2 1.5');
    expect(hollow).toHaveAttribute('fill-opacity', '0.1');
  });

  it('rejects a negative bar instead of drawing it below the base line', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <BarChart
          bars={[{ label: 'x', value: -1, state: 'complete' }]}
          ariaLabel="bad"
          formatValue={String}
        />
      )
    ).toThrow(/bars start at zero/);
  });

  it('draws signed bars on a symmetric axis and clips the outlier with a break mark and its value', () => {
    const changes = [0.3e6, 0.25e6, -8.94e6, -1.51e6, 0.4e6, 0.083e6].map((value, i) => ({
      label: `10/${18 + i}`,
      value,
      exact: `${value} CKB`,
    }));
    render(<SignedBarChart points={changes} ariaLabel="Knowledge size change" />);
    const svg = screen.getByRole('img', {
      name: `Knowledge size change; beyond the ±2M axis, drawn clipped: ${MINUS}8.94M on 10/20`,
    });
    expect(texts(svg, 'y').map((t) => t.text)).toEqual(['2M', '0', `${MINUS}2M`]);

    const clipped = svg.querySelectorAll('[data-clipped="true"]');
    expect(clipped).toHaveLength(1);
    expect(clipped[0]).toHaveAttribute('data-bar', 'down');
    expect(clipped[0].querySelector('[data-role="break"]')).toHaveAttribute('stroke', '#0c0e15');
    expect(clipped[0].querySelector('[data-role="clip-label"]')).toHaveTextContent(`${MINUS}8.94M`);
    // The clipped bar ends exactly at the axis floor.
    const rect = clipped[0].querySelector('rect') as SVGRectElement;
    expect(Number(rect.getAttribute('y')) + Number(rect.getAttribute('height'))).toBeCloseTo(84, 5);

    expect(svg.querySelectorAll('[data-bar="up"] rect')[0]).toHaveAttribute('fill', '#2edba3');
    expect(svg.querySelectorAll('[data-bar="down"] rect')[0]).toHaveAttribute('fill', '#e8555a');
  });
});

describe('ChartFigure', () => {
  it('captions the chart with a linked title, a dim sub-label and the headline', () => {
    render(
      <ChartFigure
        title="Block time"
        href="/charts/average-block-time"
        subtitle="daily average, s"
        headline="9.57"
        body={{ state: 'empty', message: 'No data yet' }}
        testId="fig"
      />
    );
    expect(screen.getByRole('link', { name: 'Block time' })).toHaveAttribute(
      'href',
      '/mainnet/charts/average-block-time'
    );
    expect(screen.getByText('daily average, s')).toHaveClass('text-text-dim');
    expect(screen.getByTestId('fig-headline')).toHaveTextContent('9.57');
    expect(screen.getByText('No data yet')).toBeInTheDocument();
  });

  it('shows an error state as an alert', () => {
    render(
      <ChartFigure
        title="Uncle rate"
        subtitle="daily"
        body={{ state: 'error', message: 'the uncle-rate response has no orphanRateTarget' }}
      />
    );
    expect(screen.getByRole('alert')).toHaveTextContent('no orphanRateTarget');
  });
});
