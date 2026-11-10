import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth, stubPointerEvents } from '@/__tests__/utils/chart-width';
import {
  BarChart,
  ChartFigure,
  LineChart,
  SignedBarChart,
  SparkChart,
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
    // No native tooltip: the caption is the readout (see "mini chart hover").
    expect(svg.querySelector('title')).toBeNull();
  });

  it('puts a reference value on a tick, emphasised, with a dashed line', () => {
    const rates = [2.42, 2.84, 2.64].map((value, i) => ({ label: `d${i}`, value }));
    render(
      <LineChart points={rates} ariaLabel="Uncle rate" reference={2.5} formatValue={String} />
    );
    const svg = screen.getByRole('img', { name: 'Uncle rate' });
    expect(texts(svg, 'y').map((t) => t.text)).toEqual(['2.3', '2.5', '2.7', '2.9']);
    const emphasised = [...svg.querySelectorAll('[data-axis="y"] text')].find(
      (t) => t.getAttribute('fill') === 'var(--c-text)'
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
    expect(svg.querySelector('[data-role="base"]')).toHaveAttribute(
      'stroke',
      'var(--c-text-ghost)'
    );

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

  // Rubbing × Song print, phase 2: counts as a sparkline of eighth blocks on the same canvas.
  it('draws a sparkline: one eighth block per bucket, a slot wide, standing on the plot floor', () => {
    const bars: Bar[] = [
      { label: '09:00', value: 610, state: 'complete' },
      { label: '10:00', value: 731, state: 'highlight' },
      { label: '11:00', value: 326, state: 'inProgress' },
    ];
    render(<SparkChart bars={bars} ariaLabel="Transactions" formatValue={String} />);
    const svg = screen.getByRole('img', { name: 'Transactions' });
    const glyphs = [...svg.querySelectorAll('[data-spark] text')];
    // from zero to the largest: 7 × 610 / 731 = 5.84 → ▇, 731 → █, 7 × 326 / 731 = 3.12 → ▄
    expect(glyphs.map((g) => g.textContent).join('')).toBe('▇█▄');
    expect(glyphs.map((g) => g.getAttribute('data-bar'))).toEqual([
      'complete',
      'highlight',
      'inProgress',
    ]);
    expect(glyphs.map((g) => g.getAttribute('fill'))).toEqual([
      'var(--c-jade)',
      'var(--c-jade-bright)',
      'var(--c-text-ghost)',
    ]);
    // 280 px: slots of (272 − 34) / 3 px; the glyph is one slot wide (0.6 em), capped so a full
    // block (1.32 em) never outgrows the 74 px plot, and its foot (0.3 em under the baseline) is
    // on the floor at y = 84
    const size = Math.min((272 - 34) / 3 / 0.6, 74 / 1.32);
    const group = svg.querySelector('[data-spark]')!;
    expect(Number(group.getAttribute('font-size'))).toBeCloseTo(size, 1);
    for (const g of glyphs) expect(Number(g.getAttribute('y')) + 0.3 * size).toBeCloseTo(84, 0);
    // no bars, no y axis: the glyph heights are the scale
    expect(svg.querySelector('rect')).toBeNull();
    expect(texts(svg, 'y')).toEqual([]);
    expect(texts(svg, 'x').map((t) => t.text)).toEqual(['09:00', '10:00', '11:00']);
  });

  it('rejects a negative count instead of drawing it', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <SparkChart
          bars={[{ label: 'x', value: -1, state: 'complete' }]}
          ariaLabel="bad"
          formatValue={String}
        />
      )
    ).toThrow('sparkline: -1 is not a count');
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
      readout: { value: `${value} CKB`, text: `${value} CKB` },
    }));
    render(<SignedBarChart points={changes} ariaLabel="Knowledge size change" />);
    const svg = screen.getByRole('img', {
      name: `Knowledge size change; beyond the ±2M axis, drawn clipped: ${MINUS}8.94M on 10/20`,
    });
    expect(texts(svg, 'y').map((t) => t.text)).toEqual(['2M', '0', `${MINUS}2M`]);

    const clipped = svg.querySelectorAll('[data-clipped="true"]');
    expect(clipped).toHaveLength(1);
    expect(clipped[0]).toHaveAttribute('data-bar', 'down');
    expect(clipped[0].querySelector('[data-role="break"]')).toHaveAttribute(
      'stroke',
      'var(--c-base-bg)'
    );
    expect(clipped[0].querySelector('[data-role="clip-label"]')).toHaveTextContent(`${MINUS}8.94M`);
    // The clipped bar ends exactly at the axis floor.
    const rect = clipped[0].querySelector('rect') as SVGRectElement;
    expect(Number(rect.getAttribute('y')) + Number(rect.getAttribute('height'))).toBeCloseTo(84, 5);

    expect(svg.querySelectorAll('[data-bar="up"] rect')[0]).toHaveAttribute(
      'fill',
      'var(--c-jade)'
    );
    expect(svg.querySelectorAll('[data-bar="down"] rect')[0]).toHaveAttribute(
      'fill',
      'var(--c-rouge)'
    );
  });

  // Visual finish V13(b), every chart: a dash means a reference, never the grid. The ±cap lines of
  // the signed axis are its grid (the scale's ends, labelled), solid like the line and bar charts'
  // grids, all in the row-rule ink #1a1f30; the zero line is the solid base, the line chart's
  // reference (a target) keeps its dash.
  it('draws every mini chart grid solid in var(--c-base-border-subtle); only the reference is dashed', () => {
    const changes = [0.3e6, -0.25e6, 0.4e6].map((value, i) => ({
      label: `10/${18 + i}`,
      value,
      readout: { value: `${value} CKB`, text: `${value} CKB` },
    }));
    const { container } = render(
      <>
        <SignedBarChart points={changes} ariaLabel="Signed" />
        <LineChart points={blockTimes} ariaLabel="Line" reference={9} formatValue={String} />
        <BarChart
          bars={[{ label: '09:00', value: 610, state: 'highlight' }]}
          ariaLabel="Bars"
          formatValue={String}
        />
      </>
    );
    const grids = [...container.querySelectorAll('[data-role="grid"]')];
    expect(grids).toHaveLength(3);
    for (const grid of grids) {
      expect(grid).toHaveAttribute('stroke', 'var(--c-base-border-subtle)');
      expect(grid).not.toHaveAttribute('stroke-dasharray');
    }
    const signed = screen.getByRole('img', { name: 'Signed' });
    expect(signed.querySelector('[data-role="base"]')).not.toHaveAttribute('stroke-dasharray');
    expect(
      screen.getByRole('img', { name: 'Line' }).querySelector('[data-role="reference"]')
    ).toHaveAttribute('stroke-dasharray', '4 3');
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

  it('keeps the title on one line, so a wide readout never makes the caption taller', () => {
    render(
      <ChartFigure
        title="DAO deposits"
        href="/charts/total-deposit"
        subtitle="daily change, CKB"
        headline="+28,116,216"
        body={{ state: 'empty', message: 'No data yet' }}
      />
    );
    const title = screen.getByRole('link', { name: 'DAO deposits' });
    expect(title).toHaveClass('truncate', 'max-w-full');
    expect(title.closest('figcaption')).toHaveClass('gap-2');
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

describe('mini chart hover', () => {
  beforeEach(() => {
    stubChartWidth(280);
    stubPointerEvents();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  // Plot x runs 34..272; three bars have centres at about 73.7, 153 and 232.3.
  const bars: Bar[] = [
    { label: '09:00', value: 610, state: 'complete' },
    { label: '10:00', value: 731, state: 'highlight' },
    { label: '11:00', value: 326, state: 'inProgress' },
  ];
  const n = (v: number) => v.toLocaleString('en-US');

  function renderBars() {
    render(
      <ChartFigure
        testId="fig"
        title="Transactions"
        subtitle="per hour"
        headline="731"
        body={{
          state: 'ready',
          chart: <BarChart bars={bars} formatValue={n} ariaLabel="Transactions" />,
        }}
      />
    );
    return screen.getByRole('img', { name: 'Transactions' });
  }

  const headline = () => screen.getByTestId('fig-headline');
  const subtitle = () => screen.getByTestId('fig-subtitle');

  it('snaps the pointer to the nearest bar, dims the rest and reads it out in the caption', () => {
    const svg = renderBars();
    fireEvent.pointerMove(svg, { clientX: 100 }); // nearer 73.7 than 153
    expect(headline()).toHaveTextContent('610');
    expect(subtitle()).toHaveTextContent('09:00');
    const groups = svg.querySelectorAll('[data-bucket]');
    expect(groups[0]).toHaveAttribute('data-hovered', 'true');
    expect(groups[0]).toHaveAttribute('opacity', '1');
    expect(groups[1]).toHaveAttribute('opacity', '0.35');
    expect(svg.querySelector('[data-bar="complete"]')).toHaveAttribute('fill-opacity', '1');
    const guide = svg.querySelector('[data-role="guide"]') as SVGLineElement;
    expect(Number(guide.getAttribute('x1'))).toBeCloseTo(73.7, 0);
    expect(guide).toHaveAttribute('stroke', 'var(--c-text-dim)');

    fireEvent.pointerMove(svg, { clientX: 120 }); // nearer 153
    expect(headline()).toHaveTextContent('731');
    expect(subtitle()).toHaveTextContent('10:00');
  });

  it('reads out the in-progress bucket too, marked as such', () => {
    const svg = renderBars();
    fireEvent.pointerMove(svg, { clientX: 260 });
    expect(headline()).toHaveTextContent('326');
    expect(subtitle()).toHaveTextContent('11:00 · in progress');
  });

  it('reverts the caption when the pointer leaves', () => {
    const svg = renderBars();
    fireEvent.pointerMove(svg, { clientX: 100 });
    fireEvent.pointerLeave(svg);
    expect(headline()).toHaveTextContent('731');
    expect(subtitle()).toHaveTextContent('per hour');
    expect(svg.querySelector('[data-role="guide"]')).toBeNull();
    expect(svg.querySelector('[data-hovered="true"]')).toBeNull();
  });

  // R6 replaces "a tapped bucket stays until the next tap elsewhere": a tap pins the bucket in a
  // readout line under the chart, and only a second tap on it (or Escape) lets it go.
  it('a tap pins the nearest bucket in the line under the chart; a second tap or Escape clears it (R6)', () => {
    const svg = renderBars();
    const readout = screen.getByRole('status', { name: 'Transactions readout' });
    expect(svg.compareDocumentPosition(readout) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(readout).toHaveTextContent(/^tap or hover$/);
    // a touch tap: down, the pointer leaves on lift, the focus it causes, then the click
    fireEvent.pointerDown(svg, { clientX: 100, pointerType: 'touch' });
    fireEvent.pointerLeave(svg, { pointerType: 'touch' });
    fireEvent.focus(svg);
    fireEvent.click(svg, { clientX: 100 });
    expect(readout).toHaveTextContent(/^selected 09:00 · 610/);
    expect(headline()).toHaveTextContent('610');
    expect(subtitle()).toHaveTextContent('09:00');
    // a tap elsewhere on the page keeps it
    act(() => {
      fireEvent.pointerDown(document.body, { pointerType: 'touch' });
    });
    expect(headline()).toHaveTextContent('610');
    // the second tap on the same bucket lets go
    fireEvent.click(svg, { clientX: 100 });
    expect(headline()).toHaveTextContent('731');
    expect(readout).toHaveTextContent(/^tap or hover$/);
    // a mouse: the hover reads over the pin, leaving shows the pin again; Escape clears it
    fireEvent.click(svg, { clientX: 120 });
    fireEvent.pointerMove(svg, { clientX: 260 });
    expect(readout).toHaveTextContent(/^11:00 · in progress · 326/);
    fireEvent.pointerLeave(svg);
    expect(readout).toHaveTextContent(/^selected 10:00 · 731/);
    act(() => {
      fireEvent.keyDown(window, { key: 'Escape' });
    });
    expect(readout).toHaveTextContent(/^tap or hover$/);
    expect(headline()).toHaveTextContent('731');
  });

  it('is focusable: focus reads the last bucket, arrows move, Escape and blur revert', () => {
    const svg = renderBars();
    expect(svg).toHaveAttribute('tabindex', '0');
    fireEvent.focus(svg);
    expect(headline()).toHaveTextContent('326');
    fireEvent.keyDown(svg, { key: 'ArrowLeft' });
    expect(headline()).toHaveTextContent('731');
    expect(screen.getByTestId('fig-readout')).toHaveTextContent('10:00: 731');
    expect(screen.getByTestId('fig-readout')).toHaveAttribute('aria-live', 'polite');
    fireEvent.keyDown(svg, { key: 'Home' });
    expect(headline()).toHaveTextContent('610');
    fireEvent.keyDown(svg, { key: 'ArrowLeft' });
    expect(headline()).toHaveTextContent('610');
    fireEvent.keyDown(svg, { key: 'Escape' });
    expect(headline()).toHaveTextContent('731');
    fireEvent.keyDown(svg, { key: 'End' });
    expect(headline()).toHaveTextContent('326');
    fireEvent.blur(svg);
    expect(headline()).toHaveTextContent('731');
  });

  it('enlarges the hovered point of a line', () => {
    const points = [9.1, 8.6, 8.8, 10.9, 9.57].map((value, i) => ({
      label: `10/${19 + i}`,
      value,
    }));
    render(
      <ChartFigure
        testId="fig"
        title="Block time"
        subtitle="daily average, s"
        headline="9.57"
        body={{
          state: 'ready',
          chart: (
            <LineChart points={points} formatValue={(v) => v.toFixed(2)} ariaLabel="Block time" />
          ),
        }}
      />
    );
    const svg = screen.getByRole('img', { name: 'Block time' });
    fireEvent.pointerMove(svg, { clientX: 215 }); // point 3 at x 212.5
    expect(headline()).toHaveTextContent('10.90');
    expect(subtitle()).toHaveTextContent('10/22');
    const dot = svg.querySelector('[data-role="hover-dot"]') as SVGCircleElement;
    expect(dot.getAttribute('cx')).toBe('212.5');
    expect(Number(dot.getAttribute('r'))).toBeGreaterThan(2.6);
  });

  it('reads a clipped outlier exactly, not as its clipped bar', () => {
    const points = [0.3e6, -8.94e6, -1.51e6, 0.4e6].map((value, i) => ({
      label: `10/${18 + i}`,
      value,
      readout: { value: `${value} exact`, text: `${value} exact` },
    }));
    render(
      <ChartFigure
        testId="fig"
        title="Knowledge size"
        subtitle="daily change, CKB"
        headline="+400,000"
        body={{ state: 'ready', chart: <SignedBarChart points={points} ariaLabel="Knowledge" /> }}
      />
    );
    const svg = screen.getByRole('img', { name: /^Knowledge/ });
    fireEvent.pointerMove(svg, { clientX: 125 }); // bar 1 centre at about 123
    expect(svg.querySelector('[data-hovered="true"]')).toHaveAttribute('data-clipped', 'true');
    expect(headline()).toHaveTextContent('-8940000 exact');
    expect(subtitle()).toHaveTextContent('10/19');
  });

  it('draws nothing extra and needs no figure when used on its own', () => {
    render(<BarChart bars={bars} formatValue={n} ariaLabel="Alone" />);
    const svg = screen.getByRole('img', { name: 'Alone' });
    fireEvent.pointerMove(svg, { clientX: 100 });
    expect(svg.querySelector('[data-role="guide"]')).not.toBeNull();
  });
});
