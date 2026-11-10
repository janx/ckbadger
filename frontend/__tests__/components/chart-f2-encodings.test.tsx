import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { Chart } from '@/components/ui/chart';

// F2 charts group: the chart encodings of the Charts / ChartDetail boards the shared Chart lacked
// (CH-09, CH-12, CH-15, CH-22, CH-23, CH-24, CH-35, CH-58). Every one is opt-in.

const daily = ['2024-01-01', '2024-06-01', '2025-01-01', '2025-06-01'].map((label, i) => ({
  label,
  values: { primary: ['1', '2.5', '3', '2'][i], real: ['0.5', '1', '1.5', '1'][i] },
}));
const series = [{ key: 'primary', label: 'Rate', color: 'var(--c-jade)' }];
const text = (container: HTMLElement) =>
  [...container.querySelectorAll('svg text')].map((node) => node.textContent);

describe('shared Chart encodings for the charts boards', () => {
  beforeEach(() => stubChartWidth(600));
  afterEach(() => vi.restoreAllMocks());

  it('pins a share chart to a fixed 0–100 domain instead of rounding up to 150 (CH-15)', () => {
    // Eight bands each rounded to two decimals can sum to 100.02: niceTicks alone then tops at 150.
    const shares = [{ label: '2024-01-01', values: { a: '60', b: '40.02' } }];
    const { container } = render(
      <Chart
        title="HODL"
        subtitle="share"
        data={shares}
        series={[
          { key: 'a', label: 'A', color: 'var(--c-jade)' },
          { key: 'b', label: 'B', color: '#21a77f' },
        ]}
        unit="%"
        kind="stacked"
        yDomain={[0, 100]}
        formatValue={(v) => v}
      />
    );
    expect(text(container)).toContain('100 %');
    expect(text(container)).not.toContain('150 %');
  });

  it('draws a dashed reference line at a target value, inside the axis (CH-12)', () => {
    const { container } = render(
      <Chart
        title="Uncle rate"
        subtitle="daily"
        data={daily}
        series={series}
        unit="%"
        referenceLines={[{ value: 4, label: 'target 4 %' }]}
        formatValue={(v) => v}
      />
    );
    const line = container.querySelector('[data-reference-line="target 4 %"] line')!;
    expect(line.getAttribute('stroke-dasharray')).toBe('3 3');
    expect(text(container)).toContain('target 4 %');
    // The axis grows to hold the target even though no point reaches it.
    expect(text(container).some((t) => t === '4 %')).toBe(true);
  });

  it('keys a target line above the plot, clear of the data and the hardfork names (C#16)', () => {
    // Responsive review C#16: "target 2.50 %" sat on the noisy data line at the right edge.
    const { container } = render(
      <Chart
        title="Uncle rate"
        subtitle="daily"
        data={daily}
        series={series}
        unit="%"
        referenceLines={[{ value: 2.5, label: 'target 2.50 %' }]}
        markers={[{ index: 3, label: 'Meepo', plotLabel: 'Meepo', href: '/hardforks' }]}
        formatValue={(v) => v}
      />
    );
    const key = container.querySelector('[data-reference-key="target 2.50 %"]')!;
    expect(key.querySelector('text')).toHaveTextContent('target 2.50 %');
    expect(key.querySelector('line')!.getAttribute('stroke-dasharray')).toBe('3 3');
    // The plot starts below the key: every grid line and the marker's name are under it.
    const keyY = Number(key.querySelector('text')!.getAttribute('y'));
    const gridYs = [
      ...container.querySelectorAll('svg line[stroke="var(--c-base-border-subtle)"]'),
    ].map((n) => Number(n.getAttribute('y1')));
    expect(Math.min(...gridYs) - keyY).toBeGreaterThanOrEqual(10);
    const marker = [...container.querySelectorAll('svg text')].find(
      (n) => n.textContent === 'Meepo'
    )!;
    expect(Number(marker.getAttribute('y')) - keyY).toBeGreaterThanOrEqual(10);
    // The line itself carries no label in the plot.
    expect(container.querySelector('[data-reference-line="target 2.50 %"] text')).toBeNull();
  });

  it('labels the x axis at the given ticks instead of only the first and last label (CH-24)', () => {
    const { container } = render(
      <Chart
        title="Hash rate"
        subtitle="daily"
        data={daily}
        series={series}
        unit="PH/s"
        xTicks={[
          { index: 0, label: '2024' },
          { index: 2, label: '2025' },
        ]}
        formatValue={(v) => v}
      />
    );
    const ticks = [...container.querySelectorAll('[data-x-tick]')].map((n) => n.textContent);
    expect(ticks).toEqual(['2024', '2025']);
    const visible = [...container.querySelectorAll('svg text:not([visibility="hidden"])')].map(
      (node) => node.textContent
    );
    expect(visible).not.toContain('2025-06-01');
    expect(visible).not.toContain('2024-01-01');
  });

  it('marks the last point and the hovered point, with an in-plot reading (CH-12, CH-22)', () => {
    const { container } = render(
      <Chart
        title="Hash rate"
        subtitle="daily"
        data={daily}
        series={series}
        unit="PH/s"
        pointDots
        formatLabel={(label) => `day ${label}`}
        formatValue={(v) => `${v}!`}
      />
    );
    expect(container.querySelector('[data-end-dot="primary"]')).not.toBeNull();
    expect(container.querySelector('[data-hover-dot]')).toBeNull();
    fireEvent.keyDown(container.querySelector('svg[role="img"]')!, { key: 'ArrowLeft' });
    expect(container.querySelector('[data-hover-dot="primary"]')).not.toBeNull();
    const reading = container.querySelector('[data-hover-reading]')!;
    expect(reading.textContent).toContain('day 2025-01-01');
    expect(reading.textContent).toContain('3! PH/s');
  });

  it('draws a series dashed or as a bare line when asked (CH-12 real inflation, CH-58)', () => {
    const { container } = render(
      <Chart
        title="Peers"
        subtitle="daily"
        data={daily}
        series={[
          { key: 'primary', label: 'Reachable', color: 'var(--c-jade)' },
          {
            key: 'real',
            label: 'Verified',
            color: 'var(--c-text-bright)',
            dashed: true,
            lineOnly: true,
          },
        ]}
        unit="peers"
        kind="area"
        formatValue={(v) => v}
      />
    );
    const filled = [...container.querySelectorAll('path[fill="var(--c-text-bright)"]')];
    expect(filled).toHaveLength(0);
    const dashed = container.querySelector('path[stroke="var(--c-text-bright)"]')!;
    expect(dashed.getAttribute('stroke-dasharray')).toBe('4 3');
    // visual finish V13: the filled series is a jade light (a gradient), the bare one has none
    const areas = [...container.querySelectorAll('path[fill^="url(#"]')];
    expect(areas).toHaveLength(1);
    const id = areas[0].getAttribute('fill')!.slice(5, -1);
    const stops = container.querySelectorAll(`linearGradient[id="${id}"] stop`);
    expect([...stops].map((stop) => stop.getAttribute('stop-color'))).toEqual([
      'var(--c-jade)',
      'var(--c-jade)',
    ]);
  });

  it('gives a few labelled buckets slot-wide bars (CH-09)', () => {
    const buckets = ['<100', '100+', '1k+'].map((label, i) => ({
      label,
      values: { primary: String(i + 1) },
    }));
    const { container } = render(
      <Chart
        title="Cell size"
        subtitle="buckets"
        data={buckets}
        series={series}
        unit="cells"
        kind="bars"
        slotBars
        formatValue={(v) => v}
      />
    );
    const widths = [...container.querySelectorAll('rect[fill="var(--c-jade)"]')].map((r) =>
      Number(r.getAttribute('width'))
    );
    expect(widths).toHaveLength(3);
    expect(Math.min(...widths)).toBeGreaterThan(20);
  });

  it('labels hardfork markers in the plot and can drop the legend row under it (CH-23)', () => {
    const { container, queryByRole } = render(
      <Chart
        title="Hash rate"
        subtitle="daily"
        data={daily}
        series={series}
        unit="PH/s"
        markers={[{ index: 1, label: 'Mirana', plotLabel: 'Mirana', href: '/hardforks' }]}
        hideMarkerLegend
        formatValue={(v) => v}
      />
    );
    expect(text(container)).toContain('Mirana');
    expect(queryByRole('link', { name: /Mirana/ })).toBeNull();
  });

  it('drops an x tick whose label would collide with the one before it (visual pass)', () => {
    stubChartWidth(200);
    const near = ['a', 'b', 'c', 'd'].map((label, i) => ({
      label,
      values: { primary: String(i) },
    }));
    const { container } = render(
      <Chart
        title="Buckets"
        subtitle="ticks"
        data={near}
        series={series}
        unit="x"
        xTicks={[
          { index: 0, label: '10k-100k' },
          { index: 1, label: '100k-1m' },
          { index: 3, label: '>=1m' },
        ]}
        formatValue={(v) => v}
      />
    );
    const ticks = [...container.querySelectorAll('[data-x-tick]')].map((n) => n.textContent);
    expect(ticks).toEqual(['10k-100k', '>=1m']);
  });

  it('keeps every label of a categorical axis, on a second row when one cannot hold them', () => {
    // Responsive review C#6: at 640 the Cell size card showed three of its six bucket labels.
    stubChartWidth(284);
    const labels = ['<100', '100-1k', '1k-10k', '10k-100k', '100k-1m', '>=1m'];
    const buckets = labels.map((label, i) => ({ label, values: { primary: String(6 - i) } }));
    const { container } = render(
      <Chart
        title="Cell size"
        subtitle="buckets"
        data={buckets}
        series={series}
        unit="cells"
        kind="bars"
        height={120}
        slotBars
        categoricalX
        xTicks={labels.map((label, index) => ({ index, label }))}
        formatValue={(v) => v}
      />
    );
    const ticks = [...container.querySelectorAll('[data-x-tick]')];
    expect(ticks.map((n) => n.textContent)).toEqual(labels);
    const rows = new Set(ticks.map((n) => n.getAttribute('y')));
    expect(rows.size).toBe(2);
    // The second row is added below the plot, not taken from it.
    expect(container.querySelector('svg[role="img"]')).toHaveAttribute('height', '132');
  });

  it('refuses a categorical axis that does not name every bar', () => {
    stubChartWidth(600);
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const buckets = ['a', 'b', 'c'].map((label) => ({ label, values: { primary: '1' } }));
    expect(() =>
      render(
        <Chart
          title="Cell size"
          subtitle="buckets"
          data={buckets}
          series={series}
          unit="cells"
          kind="bars"
          slotBars
          categoricalX
          xTicks={[
            { index: 0, label: 'a' },
            { index: 2, label: 'c' },
          ]}
          formatValue={(v) => v}
        />
      )
    ).toThrow(/categorical axis/);
  });

  it('refuses a fixed gutter narrower than its longest tick, instead of cutting the tick', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const weeks = ['2024-01-01', '2024-01-08'].map((label, i) => ({
      label,
      values: { count: ['8000', '120'][i] },
    }));
    expect(() =>
      render(
        <Chart
          title="Transactions per week"
          subtitle="weekly"
          data={weeks}
          series={[{ key: 'count', label: 'Transactions', color: 'var(--c-jade)' }]}
          unit="transactions"
          kind="bars"
          plotLeft={100}
          formatValue={(v) => v}
        />
      )
    ).toThrow(/plotLeft 100.*"10k transactions" needs 104/);
  });

  it('keys many bands with a compact swatch legend instead of headline numbers (C#5, C#9)', () => {
    // Responsive review C#5: 21 script bands were read out as right-aligned 18 px numbers before
    // the chart, ~800 px tall at 390, with no swatch tying a band to its name.
    const names = Array.from({ length: 21 }, (_, i) => `Script ${i + 1}`);
    const bands = names.map((label, i) => ({
      key: `s${i}`,
      label,
      color: `#${(0x203040 + i * 0x050403).toString(16)}`,
    }));
    const rows = ['2024-01-01', '2024-01-02'].map((label, d) => ({
      label,
      values: Object.fromEntries(bands.map((b, i) => [b.key, String((i + 1) * (d + 1))])),
    }));
    const { container } = render(
      <Chart
        title="Occupied common knowledge"
        subtitle="daily"
        data={rows}
        series={bands}
        unit="CKB"
        kind="stacked"
        seriesLegend
        formatValue={(v) => v}
      />
    );
    const legend = screen.getByRole('list', { name: 'Occupied common knowledge legend' });
    const items = within(legend).getAllByRole('listitem');
    expect(items).toHaveLength(21);
    items.forEach((item, i) => {
      // Band order, each with its band's colour, its name and its latest value.
      expect(item.querySelector('[data-swatch]')).toHaveStyle({ backgroundColor: bands[i].color });
      expect(item).toHaveTextContent(`${names[i]}${String((i + 1) * 2)} CKB`);
    });
    expect(legend).toHaveClass('text-body');
    // The legend follows the plot, and the caption carries no headline numbers.
    const plot = container.querySelector('svg[role="img"]')!;
    expect(plot.compareDocumentPosition(legend) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(container.querySelector('figcaption .text-\\[18px\\]')).toBeNull();
  });

  it('gives bars a hover dot and reading but no end dot (visual pass)', () => {
    const { container } = render(
      <Chart
        title="Deposits"
        subtitle="daily"
        data={daily}
        series={series}
        unit="CKB"
        kind="bars"
        pointDots
        formatValue={(v) => v}
      />
    );
    expect(container.querySelector('[data-end-dot]')).toBeNull();
    fireEvent.keyDown(container.querySelector('svg[role="img"]')!, { key: 'ArrowLeft' });
    expect(container.querySelector('[data-hover-dot="primary"]')).not.toBeNull();
    expect(container.querySelector('[data-hover-reading]')).toHaveTextContent('3 CKB');
  });
});

// Hierarchy v3 (DAO page, R13): a chart named inside its plot, with its round values inside the
// plot and its latest value at the line's end — no caption, no axis column. All opt-in.
describe('Chart named inside its plot (hierarchy v3)', () => {
  beforeEach(() => stubChartWidth(600));
  afterEach(() => vi.restoreAllMocks());
  const ar = ['1.0000', '1.0050', '1.0100', '1.0150'].map((v, i) => ({
    label: `2019-11-1${6 + i}`,
    values: { ar: v },
  }));
  const props = {
    title: 'AR by date',
    subtitle: 'AR at the close of every complete day',
    data: ar,
    series: [{ key: 'ar', label: 'AR', assetClass: 'dao' as const }],
    unit: '',
    kind: 'line' as const,
    height: 200,
    formatValue: (v: string) => v,
  };

  it('writes the name inside the plot instead of a caption', () => {
    const { container } = render(<Chart {...props} plotName="AR, the accumulated rate" />);
    expect(container.querySelector('figcaption')).toBeNull();
    const name = container.querySelector('svg [data-role="chart-name"]')!;
    expect(name).toHaveTextContent('AR, the accumulated rate');
    expect(name).toHaveAttribute('font-size', '11');
    // The accessible name still carries the title.
    expect(screen.getByRole('img', { name: /^AR by date/ })).toBeInTheDocument();
  });

  it('draws the round values inside the plot from its left edge, not in a gutter', () => {
    const { container } = render(<Chart {...props} plotName="AR" insideYTicks />);
    const ticks = Array.from(container.querySelectorAll('svg [data-role="y-tick"]'));
    // 1.000 … 1.015 in steps of 0.005: the lowest is the baseline and 1.015 the name's band.
    expect(ticks.map((t) => t.textContent!.trim())).toEqual(['1.005', '1.010']);
    for (const t of ticks) {
      expect(t).toHaveAttribute('x', '8');
      expect(t).not.toHaveAttribute('text-anchor', 'end');
    }
    // the line starts at the plot's left edge (no gutter)
    const line = container.querySelector('svg path[fill="none"]')!.getAttribute('d')!;
    expect(line.startsWith('M0,')).toBe(true);
  });

  it("writes the latest value at the line's end", () => {
    const { container } = render(
      <Chart {...props} plotName="AR" endLabel={(v) => v.slice(0, 5)} />
    );
    const end = container.querySelector('svg [data-role="end-label"]')!;
    expect(end).toHaveTextContent('1.015');
    expect(end).toHaveAttribute('text-anchor', 'end');
  });

  it('keeps the caption and the gutter when none of these is asked for', () => {
    const { container } = render(<Chart {...props} />);
    expect(container.querySelector('figcaption')).not.toBeNull();
    expect(container.querySelector('svg [data-role="chart-name"]')).toBeNull();
    expect(container.querySelector('svg [data-role="end-label"]')).toBeNull();
    expect(container.querySelectorAll('svg text[text-anchor="end"]').length).toBeGreaterThan(1);
  });
});

// Lane P (additive): `insideYTicks="over"` writes the ghost values over the plotted series, so a
// stacked area (60 % opaque) cannot bury them; `true` keeps them under it (the DAO page's charts).
describe('in-plot values over a filled chart', () => {
  beforeEach(() => stubChartWidth(600));
  afterEach(() => vi.restoreAllMocks());
  const held = ['2024-01-01', '2024-01-02', '2024-01-03'].map((label, i) => ({
    label,
    values: { occupied: String(100 + i * 50), free: '20' },
  }));
  const stacked = [
    { key: 'occupied', label: 'Occupied', color: 'var(--c-jade)' },
    { key: 'free', label: 'Free', color: 'var(--c-slate-ckb)' },
  ];
  const order = (insideYTicks: boolean | 'over') => {
    const { container, unmount } = render(
      <Chart
        title="CKB held"
        subtitle="occupied and free"
        data={held}
        series={stacked}
        unit="CKB"
        kind="stacked"
        height={280}
        insideYTicks={insideYTicks}
        formatValue={(v) => v}
      />
    );
    const svg = container.querySelector('svg[role="img"]')!;
    const nodes = [...svg.querySelectorAll('[data-role="y-tick"], path[fill-opacity]')];
    const kinds = nodes.map((n) => (n.getAttribute('data-role') === 'y-tick' ? 'tick' : 'area'));
    unmount();
    return kinds;
  };

  it('draws the values after the areas with "over", before them with true', () => {
    const over = order('over');
    expect(over).toContain('tick');
    expect(over.lastIndexOf('area')).toBeLessThan(over.indexOf('tick'));
    const under = order(true);
    expect(under.lastIndexOf('tick')).toBeLessThan(under.indexOf('area'));
  });
});

// Visual finish V13 (plan 2026-11-04): a chart's fill reads as light, not paint, and a dash means
// a reference, never the grid.
describe('chart material (visual finish V13)', () => {
  beforeEach(() => stubChartWidth(600));
  afterEach(() => vi.restoreAllMocks());

  const gradientOf = (container: HTMLElement, path: Element) => {
    const fill = path.getAttribute('fill')!;
    expect(fill).toMatch(/^url\(#[^)]+\)$/);
    return container.querySelector(`linearGradient[id="${fill.slice(5, -1)}"]`)!;
  };

  it('fills a single-series area with its colour at .22 at the top fading to 0 at the baseline', () => {
    const { container } = render(
      <Chart
        title="Peers"
        subtitle="daily"
        data={daily}
        series={series}
        unit="peers"
        kind="area"
        formatValue={(v) => v}
      />
    );
    const area = container.querySelector('svg[role="img"] path[fill^="url(#"]')!;
    expect(area).not.toBeNull();
    expect(area).not.toHaveAttribute('fill-opacity');
    const gradient = gradientOf(container, area);
    expect(gradient).not.toBeNull();
    // vertical, over the area's own box: the top of the fill to its foot
    expect(gradient).toHaveAttribute('x1', '0');
    expect(gradient).toHaveAttribute('y1', '0');
    expect(gradient).toHaveAttribute('x2', '0');
    expect(gradient).toHaveAttribute('y2', '1');
    const stops = [...gradient.querySelectorAll('stop')].map((stop) => [
      stop.getAttribute('offset'),
      stop.getAttribute('stop-color'),
      stop.getAttribute('stop-opacity'),
    ]);
    expect(stops).toEqual([
      ['0', 'var(--c-jade)', '0.22'],
      ['1', 'var(--c-jade)', '0'],
    ]);
  });

  it('gives every chart and every filled series its own gradient', () => {
    const two = [
      { key: 'primary', label: 'Rate', color: 'var(--c-jade)' },
      { key: 'real', label: 'Real', color: 'var(--c-aqua)' },
    ];
    const { container } = render(
      <>
        <Chart
          title="A"
          subtitle="a"
          data={daily}
          series={two}
          unit="%"
          kind="area"
          formatValue={(v) => v}
        />
        <Chart
          title="B"
          subtitle="b"
          data={daily}
          series={series}
          unit="%"
          kind="area"
          formatValue={(v) => v}
        />
      </>
    );
    const fills = [...container.querySelectorAll('path[fill^="url(#"]')].map((p) =>
      p.getAttribute('fill')
    );
    expect(fills).toHaveLength(3);
    expect(new Set(fills).size).toBe(3);
    const ids = [...container.querySelectorAll('linearGradient')].map((g) => g.id);
    expect(new Set(ids).size).toBe(ids.length);
    const second = container
      .querySelectorAll('svg[role="img"]')[0]
      .querySelectorAll('path[fill^="url(#"]')[1];
    expect(gradientOf(container, second).querySelector('stop')).toHaveAttribute(
      'stop-color',
      'var(--c-aqua)'
    );
  });

  it('keeps a stacked area a flat 60 % paint, with no gradient', () => {
    const { container } = render(
      <Chart
        title="Held"
        subtitle="occupied and free"
        data={daily}
        series={[
          { key: 'primary', label: 'Occupied', color: 'var(--c-jade)' },
          { key: 'real', label: 'Free', color: 'var(--c-slate-ckb)' },
        ]}
        unit="CKB"
        kind="stacked"
        formatValue={(v) => v}
      />
    );
    const areas = [...container.querySelectorAll('path[fill-opacity]')];
    expect(areas.map((a) => [a.getAttribute('fill'), a.getAttribute('fill-opacity')])).toEqual([
      ['var(--c-jade)', '0.6'],
      ['var(--c-slate-ckb)', '0.6'],
    ]);
    expect(container.querySelector('linearGradient')).toBeNull();
  });

  it('draws the grid solid, in or beside the plot; a target and a hardfork keep their dash', () => {
    for (const insideYTicks of [false, true]) {
      const { container, unmount } = render(
        <Chart
          title="Uncle rate"
          subtitle="daily"
          data={daily}
          series={series}
          unit="%"
          height={280}
          insideYTicks={insideYTicks}
          referenceLines={[{ value: 4, label: 'target 4 %' }]}
          markers={[{ index: 2, label: 'Meepo', plotLabel: 'Meepo', href: '/hardforks' }]}
          formatValue={(v) => v}
        />
      );
      const grid = [
        ...container.querySelectorAll('svg line[stroke="var(--c-base-border-subtle)"]'),
      ];
      expect(grid.length).toBeGreaterThan(1);
      for (const line of grid) expect(line).not.toHaveAttribute('stroke-dasharray');
      expect(container.querySelector('[data-reference-line="target 4 %"] line')).toHaveAttribute(
        'stroke-dasharray',
        '3 3'
      );
      const marker = [...container.querySelectorAll('svg g')].find(
        (g) => g.querySelector('title')?.textContent === 'Meepo'
      )!;
      expect(marker.querySelector('line')).toHaveAttribute('stroke-dasharray', '3 3');
      unmount();
    }
  });
});
