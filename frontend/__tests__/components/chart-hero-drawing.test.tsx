import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { Chart } from '@/components/ui/chart';
import { CHARTS } from '@/components/charts/chart-catalogue';
import { lineAscentPx, SVG_TEXT } from '@/lib/chart-type';
import { descentPx, tightLinePx } from '@/lib/type-metrics';

/**
 * The shared drawing's opt-ins for the chart pages on the chart-hero board (lane D1). Each is opt-in,
 * so the DAO page, the asset and script histories and the home page draw as before; the in-plot
 * ticks had no caller before the chart pages.
 */

const line = (values: readonly number[]) =>
  values.map((value, i) => ({
    label: `2026-01-${String(i + 1).padStart(2, '0')}`,
    values: { v: String(value) },
  }));

/** The y of the grid line a tick belongs to: the one whose y is nearest the tick's. */
function gridY(container: HTMLElement, tickY: number): number {
  const ys = [...container.querySelectorAll('line')]
    .filter((el) => el.getAttribute('y1') === el.getAttribute('y2'))
    .map((el) => Number(el.getAttribute('y1')));
  return ys.reduce((best, y) => (Math.abs(y - tickY) < Math.abs(best - tickY) ? y : best));
}

describe('the chart pages’ drawing (lane D1)', () => {
  it('moves an in-plot tick off the series: under its line, else to the right end (rule 4)', () => {
    stubChartWidth(480);
    // the axis runs 0, 50, 100: only the 50 is written (0 is the baseline, 100 the plot's top)
    const tick50 = (values: readonly number[]) => {
      const { container, unmount } = render(
        <Chart
          title="Ticks"
          subtitle="x"
          data={line(values)}
          series={[{ key: 'v', label: 'v' }]}
          unit="u"
          height={220}
          formatValue={(v) => v}
          plotName="u · every day"
          insideYTicks
          bareTicks
        />
      );
      const ticks = [...container.querySelectorAll('[data-role="y-tick"]')];
      expect(ticks.map((el) => el.textContent)).toEqual(['50']);
      const y = Number(ticks[0].getAttribute('y'));
      const placed = { y, line: gridY(container, y), anchor: ticks[0].getAttribute('text-anchor') };
      unmount();
      return placed;
    };
    // flat at 50 at the left end: over its line the number would lie on the series, so under it
    const under = tick50([50, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 100]);
    expect(under.y).toBeGreaterThan(under.line);
    expect(under.anchor).toBe('start');
    // crossing the line at the left end, over and under: at the right end, over its line
    const right = tick50([40, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    expect(right.y).toBeLessThan(right.line);
    expect(right.anchor).toBe('end');
    // nothing near it: over its line at the left end
    const over = tick50([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 100]);
    expect(over.y).toBeLessThan(over.line);
    expect(over.anchor).toBe('start');
  });

  it('names a stack’s bands at their right ends in solid fills, where the plot keeps three fifths (rule 5)', () => {
    stubChartWidth(1000);
    const data = ['2026-01-01', '2026-01-02'].map((label) => ({
      label,
      values: { old: '60', mid: '30', young: '10' },
    }));
    const series = [
      { key: 'old', label: '3 years or more', color: 'var(--c-jade)' },
      { key: 'mid', label: '1 to 3 years', color: 'var(--c-ramp-1)' },
      { key: 'young', label: 'under 1 month', color: 'var(--c-ramp-2)' },
    ];
    const ends = { old: '60.00%', mid: '30.00%', young: '10.00%' };
    const { container } = render(
      <Chart
        title="Ages"
        subtitle="x"
        data={data}
        series={series}
        unit="%"
        kind="stacked"
        yDomain={[0, 100]}
        formatValue={(v) => v}
        plotName="% · every day"
        insideYTicks
        bareTicks
        bandEnds={ends}
        solidBands
      />
    );
    const bands = [...container.querySelectorAll('[data-band-end]')];
    expect(bands.map((band) => band.textContent)).toEqual(
      expect.arrayContaining(['3 years or more60.00%', '1 to 3 years30.00%', 'under 1 month10.00%'])
    );
    // the names stand apart, a line each
    const ys = bands
      .map((band) => Number(band.querySelector('text')!.getAttribute('y')))
      .sort((a, b) => a - b);
    for (let i = 1; i < ys.length; i++) expect(ys[i] - ys[i - 1]).toBeGreaterThanOrEqual(15);
    const fills = [...container.querySelectorAll('path[fill-opacity]')];
    expect(fills.length).toBe(3);
    for (const fill of fills) expect(fill.getAttribute('fill-opacity')).toBe('1');
    expect(screen.queryByRole('list', { name: 'Ages key' })).toBeNull();
  });

  it('keys a stack under its plot, top band first, where the column does not fit (rule 5, 390)', () => {
    stubChartWidth(358);
    render(
      <Chart
        title="Ages"
        subtitle="x"
        data={['2026-01-01', '2026-01-02'].map((label) => ({
          label,
          values: { old: '60', young: '40' },
        }))}
        series={[
          { key: 'old', label: '3 years or more' },
          { key: 'young', label: 'under 1 month' },
        ]}
        unit="%"
        kind="stacked"
        formatValue={(v) => v}
        plotName="%"
        insideYTicks
        bareTicks
        bandEnds={{ old: '60.00%', young: '40.00%' }}
      />
    );
    const key = screen.getByRole('list', { name: 'Ages key' });
    expect([...key.querySelectorAll('li')].map((li) => li.textContent)).toEqual([
      'under 1 month40.00%',
      '3 years or more60.00%',
    ]);
  });

  it('stops the axis at its cap, cuts what is above under a gold tick and names it (rule 6)', () => {
    stubChartWidth(480);
    const values = [50.48, 17.21, ...Array.from({ length: 20 }, (_, i) => (i % 2 ? 4.05 : 3.95))];
    const { container } = render(
      <Chart
        title="Epochs"
        subtitle="x"
        data={line(values)}
        series={[{ key: 'v', label: 'v' }]}
        unit="h"
        height={220}
        formatValue={(v) => v}
        plotName="h · every epoch"
        insideYTicks
        bareTicks
        axisCap={{ value: 4.05, note: '2 epochs above this scale; the largest, #0 · 50.48 h' }}
      />
    );
    expect(container.querySelector('[data-role="axis-cap"]')).toHaveTextContent(
      '2 epochs above this scale; the largest, #0 · 50.48 h'
    );
    expect(
      [...container.querySelectorAll('[data-cut]')].map((el) => el.getAttribute('data-cut'))
    ).toEqual(['2026-01-01', '2026-01-02']);
    expect(container.querySelector('clipPath')).not.toBeNull();
    const ticks = [...container.querySelectorAll('[data-role="y-tick"]')].map((el) =>
      Number(el.textContent)
    );
    expect(Math.max(...ticks)).toBeLessThan(10);
  });

  it('draws a quiet chart’s line without the halo (rule 7)', () => {
    stubChartWidth(480);
    const { container } = render(
      <Chart
        title="Quiet"
        subtitle="x"
        data={line([1, 2, 3])}
        series={[{ key: 'v', label: 'v' }]}
        unit="u"
        formatValue={(v) => v}
        quiet
      />
    );
    expect(container.querySelector('.chart-line-halo')).toBeNull();
  });

  // The D1 hand-back, item 4: at 360 the plot's name met the fork markers' labels on
  // /charts/circulation-ratio. The plot's top takes the name's lines, as many as it is measured to
  // take, then the markers' labels a line under them, at every width.
  it('stands the fork markers’ labels under every line of the plot’s name, at every width', () => {
    const longest = CHARTS.map((chart) => chart.plotUnit).reduce((a, b) =>
      b.length > a.length ? b : a
    );
    const name = `${longest} · every complete day since 2019-11-16`;
    for (const width of [328, 390, 480, 640, 960, 1232]) {
      stubChartWidth(width);
      const { container, unmount } = render(
        <Chart
          title="Markers"
          subtitle="x"
          data={line([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])}
          series={[{ key: 'v', label: 'v' }]}
          unit="u"
          height={360}
          formatValue={(v) => v}
          markers={[
            { index: 1, label: 'Mirana', plotLabel: 'Mirana', href: '/hardforks' },
            { index: 4, label: 'Meepo', plotLabel: 'Meepo', href: '/hardforks' },
            { index: 9, label: 'Hardfork 3', plotLabel: 'Hardfork 3', href: '/hardforks' },
          ]}
          plotName={name}
          insideYTicks
          bareTicks
        />
      );
      const nameText = container.querySelector('[data-role="chart-name"]')!;
      const lines = Math.max(1, nameText.querySelectorAll('tspan').length);
      // the name's last line: its baseline and its descent
      const nameFoot =
        Number(nameText.getAttribute('y')) +
        (lines - 1) * tightLinePx(SVG_TEXT.label) +
        descentPx(SVG_TEXT.label);
      const labels = [...container.querySelectorAll('g > title')]
        .filter((title) => ['Mirana', 'Meepo', 'Hardfork 3'].includes(title.textContent ?? ''))
        .map((title) => title.parentElement!.querySelector('text')!);
      expect(labels).toHaveLength(3);
      for (const label of labels)
        expect(
          Number(label.getAttribute('y')) - lineAscentPx(SVG_TEXT.tick),
          `${width} px: ${label.textContent} under ${lines} line(s) of the name`
        ).toBeGreaterThanOrEqual(nameFoot);
      unmount();
    }
  });
});
