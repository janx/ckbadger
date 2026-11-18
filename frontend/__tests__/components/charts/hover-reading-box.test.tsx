import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { Chart, HOVER_PAD } from '@/components/ui/chart';
import { formatChartHover } from '@/components/charts/chart-model';
import { SVG_TEXT } from '@/lib/chart-type';
import { withUnit } from '@/lib/format-unit';
import { textPx } from '@/lib/type-metrics';

// Review of main..fix/readability, follow-up (the lead's Chrome check on :8190): a chart's in-plot
// hover reading ran off a phone's svg. On /charts/total-deposit at 390 (svg 358) its texts stood
// at −16.6 … 186.2, at 360 (svg 328) at −31.6 … 171.2; on /charts/total-supply at 390 the rows
// stood at −102.4 … 201.8 ("Circulating 30,897,707,188.37050575 CKB" is 304 px) and, near the
// left edge, at 77.7 … 381.9, past the right one. The box measured its widest line at the head's
// size while the rows are drawn at the reading size, and it went left of the point with no check
// that the left had the room. The reading is drawn whole or not at all: as wide as its widest line
// at that line's own size and its padding; right of the point where the svg holds it there, else
// left, else against the svg's nearer edge; where the svg cannot hold it, no box (the readout line
// and the key under the plot read the point). No size is written here: the type steps differ by
// line, and the expectations are computed from SVG_TEXT.

const days = Array.from({ length: 30 }, (_, i) =>
  new Date(Date.UTC(2026, 9, 19 + i)).toISOString().slice(0, 10)
);
/** DAO deposits: one exact CKB row of 26 glyphs ("8,176,203,931.02839268 CKB"). */
const deposits = {
  series: [{ key: 'primary', label: 'DAO deposits', color: 'var(--c-jade)' }],
  data: days.map((label) => ({ label, values: { primary: '8176203931.02839268' } })),
  kind: 'line' as const,
};
/** Supply: three named bands, the widest row of 39 glyphs ("Circulating 30,897,707,188.37050575 CKB"). */
const supply = {
  series: [
    { key: 'circulating', label: 'Circulating', color: 'var(--c-jade)' },
    { key: 'nervosdao', label: 'Nervos DAO', color: 'var(--c-gold)' },
    { key: 'burnt', label: 'Burnt', color: 'var(--c-rouge)' },
  ],
  data: days.map((label) => ({
    label,
    values: {
      circulating: '30897707188.37050575',
      nervosdao: '8176203931.02839268',
      burnt: '15214298485.14083783',
    },
  })),
  kind: 'stacked' as const,
};
/**
 * Most-utilised scripts' and assets' two histories (review of the fixes, M1): the top 20 and
 * "Others", 21 bands, each a `Chart` 260 px high with a key under the plot (chart-detail.tsx).
 */
const bandSeries = Array.from({ length: 21 }, (_, i) => ({
  key: `band${i}`,
  label: i === 20 ? 'Others' : `script ${i + 1}`,
  color: 'var(--c-jade)',
}));
const bands = {
  series: bandSeries,
  data: days.map((label) => ({
    label,
    values: Object.fromEntries(bandSeries.map((s, i) => [s.key, `${i + 1}.5`])),
  })),
  kind: 'stacked' as const,
};
type Shape = typeof deposits | typeof supply | typeof bands;

/** The texts the reading draws, as the chart writes them: the date, then each band's figure. */
function lines(shape: Shape, index: number): { text: string; size: number }[] {
  const point = shape.data[index];
  return [
    { text: point.label, size: SVG_TEXT.label },
    ...shape.series.map((s) => ({
      text: `${shape.series.length > 1 ? `${s.label} ` : ''}${withUnit(
        formatChartHover((point.values as Record<string, string>)[s.key], 'CKB'),
        'CKB'
      )}`,
      size: SVG_TEXT.reading,
    })),
  ];
}

function mount(shape: Shape, width: number, index: number, height = 360) {
  stubChartWidth(width);
  return render(
    <Chart
      title="Reading"
      subtitle="daily"
      data={shape.data}
      series={shape.series}
      unit="CKB"
      kind={shape.kind}
      height={height}
      hideCaption
      pointDots
      activeIndex={index}
      formatValue={(value) => formatChartHover(value, 'CKB')}
      // as the detail page draws a history of many bands: its key under the plot (C#5)
      seriesLegend={shape === bands}
      pinnedReadout={shape === bands ? { label: 'Occupied common knowledge' } : undefined}
    />
  );
}

/** The svg's own size, as drawn. */
function svgOf(container: HTMLElement) {
  const svg = container.querySelector('svg[role="img"]')!;
  return { width: Number(svg.getAttribute('width')), height: Number(svg.getAttribute('height')) };
}

/**
 * The reading's box and texts as drawn: the box's edges, each text's ends at its own size and its
 * baseline, its cap a size above the baseline.
 */
function readingOf(container: HTMLElement) {
  const g = container.querySelector('[data-hover-reading]');
  if (!g) return null;
  const rect = g.querySelector('rect')!;
  const x = Number(rect.getAttribute('x'));
  const y = Number(rect.getAttribute('y'));
  return {
    rect: {
      left: x,
      right: x + Number(rect.getAttribute('width')),
      top: y,
      bottom: y + Number(rect.getAttribute('height')),
    },
    texts: [...g.querySelectorAll('text')].map((t) => {
      const left = Number(t.getAttribute('x'));
      const size = Number(t.getAttribute('font-size'));
      const baseline = Number(t.getAttribute('y'));
      return {
        text: t.textContent!,
        left,
        right: left + textPx(t.textContent!.length, size),
        top: baseline - size,
        baseline,
      };
    }),
  };
}

const EPS = 1e-6;
/** The box's 1 px border is centred on its edge: half of it lies outside the rect. */
const HALF_BORDER = 0.5;
/** The reading drawn whole: its box and border inside the svg, in x and in y; its texts in its box. */
function expectWhole(container: HTMLElement, label: string) {
  const reading = readingOf(container);
  expect(reading, label).not.toBeNull();
  const { rect, texts } = reading!;
  const svg = svgOf(container);
  expect(rect.left - HALF_BORDER, `${label}: rect left`).toBeGreaterThanOrEqual(-EPS);
  expect(rect.right + HALF_BORDER, `${label}: rect right`).toBeLessThanOrEqual(svg.width + EPS);
  expect(rect.top - HALF_BORDER, `${label}: rect top`).toBeGreaterThanOrEqual(-EPS);
  expect(rect.bottom + HALF_BORDER, `${label}: rect bottom`).toBeLessThanOrEqual(svg.height + EPS);
  for (const t of texts) {
    expect(t.left, `${label}: "${t.text}" left in its rect`).toBeGreaterThanOrEqual(rect.left);
    expect(t.right, `${label}: "${t.text}" right in its rect`).toBeLessThanOrEqual(
      rect.right + EPS
    );
    expect(t.top, `${label}: "${t.text}" cap in its rect`).toBeGreaterThanOrEqual(rect.top);
    expect(t.baseline, `${label}: "${t.text}" baseline in its rect`).toBeLessThan(rect.bottom);
  }
}

describe('the in-plot hover reading is drawn whole or not at all', () => {
  afterEach(() => vi.restoreAllMocks());

  for (const [name, shape] of [
    ['DAO deposits (26 glyphs)', deposits],
    ['Supply (39 glyphs)', supply],
  ] as const)
    for (const width of [358, 328])
      it(`${name} at a ${width} px svg: inside the svg at the first, a middle and the last point, or not drawn`, () => {
        for (const index of [0, 14, 29]) {
          const label = `${name}, ${width} px, point ${index}`;
          const drawn = lines(shape, index);
          const needed =
            Math.max(...drawn.map((line) => textPx(line.text.length, line.size))) + 2 * HOVER_PAD;
          const { container, unmount } = mount(shape, width, index);
          // the texts drawn are the lines above, at those sizes, where the svg holds the box and
          // its 1 px border (half a pixel each side); none where it cannot
          expect(
            [...container.querySelectorAll('[data-hover-reading] text')].map((t) => [
              t.textContent,
              Number(t.getAttribute('font-size')),
            ]),
            label
          ).toEqual(needed + 1 <= width ? drawn.map((line) => [line.text, line.size]) : []);
          if (needed + 1 <= width) expectWhole(container, label);
          else expect(readingOf(container), label).toBeNull();
          unmount();
        }
      });

  it('draws no box where the svg cannot hold its widest line', () => {
    const widest = Math.max(...lines(deposits, 14).map((l) => textPx(l.text.length, l.size)));
    const { container } = mount(deposits, Math.floor(widest), 14);
    expect(readingOf(container)).toBeNull();
    // the hover dot still marks the point
    expect(container.querySelector('[data-hover-dot]')).not.toBeNull();
  });

  it('stands right of the point at 1232, left of it at the last point, whole', () => {
    for (const index of [0, 14, 29]) {
      const { container, unmount } = mount(deposits, 1232, index);
      expectWhole(container, `1232 px, point ${index}`);
      const cx = Number(container.querySelector('[data-hover-dot]')!.getAttribute('cx'));
      const { rect } = readingOf(container)!;
      if (index < 29) expect(rect.left, `point ${index}`).toBeGreaterThan(cx);
      else expect(rect.right, `point ${index}`).toBeLessThan(cx);
      unmount();
    }
  });

  // Review of the fixes (2026-11-19, M1; the lead saw it in Chrome at 1280): the box was bounded
  // in x only. On /charts/most-utilized-scripts and /charts/most-utilized-assets each history has
  // 21 bands in a 260 px chart, and the reading's rect stood 358 px tall from y 20, its last 8 rows
  // (and "Others") past the svg's foot, clipped. Where the svg cannot hold the box in y either, no
  // box is drawn; nothing else changes: the key under the plot states every band at the point.
  it('draws no reading for 21 bands in a 260 px chart; the key under the plot states every band', () => {
    for (const index of [0, 14, 29]) {
      const label = `21 bands, 260 px, point ${index}`;
      const { container, unmount } = mount(bands, 1232, index, 260);
      expect(svgOf(container).height, label).toBe(260);
      expect(readingOf(container), label).toBeNull();
      expect(container.querySelector('[data-hover-dot]'), label).not.toBeNull();
      const key = [...container.querySelectorAll('ul[aria-label="Reading legend"] li')];
      const squeezed = (text: string) => text.replace(/\s+/g, '');
      expect(
        key.map((li) => squeezed(li.textContent!)),
        label
      ).toEqual(
        bandSeries.map((s) =>
          squeezed(
            `${s.label}${withUnit(formatChartHover(bands.data[index].values[s.key], 'CKB'), 'CKB')}`
          )
        )
      );
      unmount();
    }
  });

  it('stands in an svg exactly as tall as its box and border, and in none a pixel shorter', () => {
    // where the svg has room, the 21 bands' box is drawn whole; its top and height do not depend
    // on the svg's height
    const roomy = mount(bands, 1232, 14, 1000);
    expectWhole(roomy.container, '21 bands, 1000 px');
    const { rect } = readingOf(roomy.container)!;
    roomy.unmount();
    const exact = rect.bottom + HALF_BORDER;
    const fits = mount(bands, 1232, 14, exact);
    expect(readingOf(fits.container)!.rect, `${exact} px`).toEqual(rect);
    expectWhole(fits.container, `21 bands, ${exact} px`);
    fits.unmount();
    const short = mount(bands, 1232, 14, exact - 1);
    expect(readingOf(short.container), `${exact - 1} px`).toBeNull();
  });

  it('stands in an svg exactly as wide as its box and border, and in none a pixel narrower', () => {
    const drawn = lines(deposits, 14);
    const needed =
      Math.max(...drawn.map((line) => textPx(line.text.length, line.size))) + 2 * HOVER_PAD;
    const fits = mount(deposits, needed + 2 * HALF_BORDER, 14);
    expectWhole(fits.container, `${needed + 1} px wide`);
    fits.unmount();
    const short = mount(deposits, needed + 2 * HALF_BORDER - 1, 14);
    expect(readingOf(short.container), `${needed} px wide`).toBeNull();
  });
});
