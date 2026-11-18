import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { alphaOf, declaration, inTheme, ruleOf, stopsOf } from '@/__tests__/utils/css-paint';
import { BarChart, LineChart, SignedBarChart } from '@/components/home/mini-chart';
import { AlignedBar } from '@/components/ui/aligned-bar';
import { Chart } from '@/components/ui/chart';
import type { ThemeName } from '@/lib/palette';
import { resetThemeForTests, THEME_STORAGE_KEY } from '@/lib/theme';

// Readability R1 (Gate F0; the owner: by night the scan is cut into a figure's fills, and no layer
// lies over the text). By night the scan is a property of a fill: 1 px in every 3 CSS px, the
// ground showing through the hit row at the strength the window-wide layer had on a fill (22 %),
// never over a word. An HTML fill takes the class `.scan-fill` (a mask image); a chart's areas and
// bars take one SVG mask of the same rhythm, drawn 1:1 so its 3 px stay 3 px on the screen. By day
// there is none: the ruled paper is the window's.

function setTheme(theme: ThemeName) {
  localStorage.setItem(THEME_STORAGE_KEY, theme);
  resetThemeForTests();
}

/** The rhythm of the SVG mask a fill references: its units, its period and its rows. */
function scanMaskOf(el: Element) {
  const svg = el.closest('svg')!;
  const ref = (value: string | null) => /^url\(#(.+)\)$/.exec(value ?? '')?.[1];
  const maskId = ref(el.getAttribute('mask'));
  expect(maskId, 'a url(#…) mask').toBeDefined();
  const mask = svg.querySelector(`mask[id="${maskId}"]`)!;
  expect(mask, `mask #${maskId} in the same svg`).not.toBeNull();
  const rows = svg.querySelector(
    `pattern[id="${ref(mask.querySelector('rect')!.getAttribute('fill'))}"]`
  )!;
  expect(rows, 'the mask is filled with a pattern').not.toBeNull();
  return {
    alpha: mask.getAttribute('style'),
    units: [mask.getAttribute('maskUnits'), rows.getAttribute('patternUnits')],
    period: [rows.getAttribute('width'), rows.getAttribute('height')],
    rows: [...rows.querySelectorAll('rect')].map((r) => ({
      y: Number(r.getAttribute('y') ?? 0),
      height: Number(r.getAttribute('height')),
      opacity: Number(r.getAttribute('fill-opacity') ?? 1),
      fill: r.getAttribute('fill'),
    })),
  };
}

/** The night's mask: an alpha mask with no colour, 2 px kept whole and 1 px at 78 %, every 3 px. */
const NIGHT_SCAN = {
  alpha: 'mask-type: alpha;',
  units: ['userSpaceOnUse', 'userSpaceOnUse'],
  period: ['3', '3'],
  rows: [
    { y: 0, height: 2, opacity: 1, fill: null },
    { y: 2, height: 1, opacity: 0.78, fill: null },
  ],
};

const masked = (el: Element) => el.closest('[mask]') !== null;

describe('the scan cut into a fill (readability R1)', () => {
  beforeEach(() => stubChartWidth(600));
  afterEach(() => {
    localStorage.clear();
    resetThemeForTests();
    document.documentElement.setAttribute('data-theme', 'dark');
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('cuts an HTML fill by night (`.scan-fill`), 1 px in 3 at 22 %, and leaves it whole by day', () => {
    const rule = ruleOf('scan-fill');
    const image = declaration(rule, 'mask-image');
    // the standard property and its -webkit- twin, one value
    expect(image).toBeDefined();
    expect(declaration(rule, '-webkit-mask-image')).toBe(image);
    // the rule sets the mask and nothing else: no colour, no size, no position
    expect(
      rule
        .split(';')
        .map((d) => d.split(':')[0].trim())
        .filter(Boolean)
    ).toEqual(['-webkit-mask-image', 'mask-image']);
    const night = stopsOf(inTheme('dark', image!));
    expect(night.map((s) => s.at)).toEqual([0, 2, 2, 3]);
    expect(night.map((s) => alphaOf(s.colour))).toEqual([1, 1, 0.78, 0.78]);
    expect(inTheme('light', image!)).toBe('none');
  });

  it('puts it on the fill of a list bar, not on its track or its figure', () => {
    const { container } = render(
      <AlignedBar
        label="Size, bytes"
        scale={4000}
        segments={[
          { key: 'overhead', label: 'overhead', value: 800, tone: 'overhead' },
          { key: 'user', label: 'user transactions', value: 1200, tone: 'data' },
        ]}
      />
    );
    const segments = [...container.querySelectorAll('[data-segment]')];
    expect(segments).toHaveLength(2);
    for (const s of segments) expect(s).toHaveClass('scan-fill');
    expect(container.querySelector('[data-track]')).not.toHaveClass('scan-fill');
    expect(screen.getByText('2,000')).not.toHaveClass('scan-fill');
  });

  const days = ['2025-01-01', '2025-01-02', '2025-01-03', '2025-01-04'].map((label, i) => ({
    label,
    values: { a: ['1', '3', '2', '4'][i], b: ['2', '1', '1', '2'][i] },
  }));

  it('masks an area chart’s fill by night, never its line, its ticks or its labels', () => {
    setTheme('dark');
    const { container } = render(
      <Chart
        title="Peers"
        subtitle="daily"
        data={days}
        series={[{ key: 'a', label: 'Peers', color: 'var(--c-jade)' }]}
        unit="peers"
        kind="area"
        formatValue={(v) => v}
      />
    );
    const area = container.querySelector('path[fill^="url(#chart-area"]')!;
    expect(area).not.toBeNull();
    expect(scanMaskOf(area)).toEqual(NIGHT_SCAN);
    const line = container.querySelector('path[fill="none"][stroke="var(--c-jade)"]')!;
    expect(masked(line)).toBe(false);
    const texts = [...container.querySelectorAll('svg text')];
    expect(texts.length).toBeGreaterThan(0);
    for (const t of texts) expect(masked(t), t.textContent ?? '').toBe(false);
  });

  it('masks a stacked chart’s bands by night', () => {
    setTheme('dark');
    const { container } = render(
      <Chart
        title="Shares"
        subtitle="daily"
        data={days}
        series={[
          { key: 'a', label: 'A', color: 'var(--c-jade)' },
          { key: 'b', label: 'B', color: 'var(--c-aqua)' },
        ]}
        unit="%"
        kind="stacked"
        formatValue={(v) => v}
      />
    );
    const bands = [...container.querySelectorAll('path[fill-opacity="0.6"]')];
    expect(bands).toHaveLength(2);
    for (const band of bands) expect(scanMaskOf(band)).toEqual(NIGHT_SCAN);
  });

  it('masks a bar chart’s bars by night, never the hovered point', () => {
    setTheme('dark');
    const { container } = render(
      <Chart
        title="Blocks"
        subtitle="daily"
        data={days}
        series={[{ key: 'a', label: 'Blocks', color: 'var(--c-jade)' }]}
        unit="blocks"
        kind="bars"
        pointDots
        activeIndex={1}
        formatValue={(v) => v}
      />
    );
    const bars = [...container.querySelectorAll('rect[fill="var(--c-jade)"]')];
    expect(bars).toHaveLength(4);
    // one mask over a series' bars (a mask per bar would be a layer per bar)
    const groups = new Set(bars.map((bar) => bar.closest('[mask]')));
    expect([...groups].map((g) => g?.tagName)).toEqual(['g']);
    expect(scanMaskOf([...groups][0]!)).toEqual(NIGHT_SCAN);
    const dot = container.querySelector('[data-hover-dot]')!;
    expect(dot).not.toBeNull();
    expect(masked(dot)).toBe(false);
    for (const t of container.querySelectorAll('svg text')) expect(masked(t)).toBe(false);
  });

  it('draws no mask by day, and none on a line chart', () => {
    setTheme('light');
    const day = render(
      <Chart
        title="Peers"
        subtitle="daily"
        data={days}
        series={[{ key: 'a', label: 'Peers', color: 'var(--c-jade)' }]}
        unit="peers"
        kind="area"
        formatValue={(v) => v}
      />
    );
    expect(day.container.querySelector('[mask], mask')).toBeNull();
    day.unmount();
    setTheme('dark');
    const line = render(
      <Chart
        title="Rate"
        subtitle="daily"
        data={days}
        series={[{ key: 'a', label: 'Rate', color: 'var(--c-jade)' }]}
        unit="%"
        formatValue={(v) => v}
      />
    );
    expect(line.container.querySelector('[mask], mask')).toBeNull();
  });

  it('masks the home charts’ solid bars by night, never an unfinished bar’s outline or a label', () => {
    setTheme('dark');
    render(
      <BarChart
        bars={[
          { label: '09:00', value: 3, state: 'complete' },
          { label: '10:00', value: 5, state: 'highlight' },
          { label: '11:00', value: 1, state: 'inProgress' },
        ]}
        ariaLabel="Transactions"
        formatValue={String}
      />
    );
    const svg = screen.getByRole('img', { name: 'Transactions' });
    for (const state of ['complete', 'highlight'])
      expect(scanMaskOf(svg.querySelector(`rect[data-bar="${state}"]`)!)).toEqual(NIGHT_SCAN);
    expect(masked(svg.querySelector('rect[data-bar="inProgress"]')!)).toBe(false);
    for (const t of svg.querySelectorAll('text')) expect(masked(t)).toBe(false);
  });

  it('masks the signed bars, never the clipped bar’s value', () => {
    setTheme('dark');
    const changes = [0.3e6, -8.94e6, 0.4e6].map((value, i) => ({
      label: `10/${18 + i}`,
      value,
      readout: { value: `${value} CKB`, text: `${value} CKB` },
    }));
    render(<SignedBarChart points={changes} ariaLabel="Change" />);
    const svg = screen.getByRole('img', { name: /^Change/ });
    const rects = [...svg.querySelectorAll('[data-bar] rect')];
    expect(rects).toHaveLength(3);
    for (const r of rects) expect(scanMaskOf(r)).toEqual(NIGHT_SCAN);
    expect(masked(svg.querySelector('[data-role="clip-label"]')!)).toBe(false);
    expect(masked(svg.querySelector('[data-role="break"]')!)).toBe(false);
  });

  it('draws no mask on the home line chart, and none on the bars by day', () => {
    setTheme('dark');
    render(
      <LineChart
        points={[1, 2, 3].map((value, i) => ({ label: `d${i}`, value }))}
        ariaLabel="Block time"
        formatValue={String}
      />
    );
    expect(
      screen.getByRole('img', { name: 'Block time' }).querySelector('[mask], mask')
    ).toBeNull();
    setTheme('light');
    render(
      <BarChart
        bars={[{ label: '09:00', value: 3, state: 'complete' }]}
        ariaLabel="By day"
        formatValue={String}
      />
    );
    expect(screen.getByRole('img', { name: 'By day' }).querySelector('[mask], mask')).toBeNull();
  });
});
