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
type Shape = typeof deposits | typeof supply;

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

function mount(shape: Shape, width: number, index: number) {
  stubChartWidth(width);
  return render(
    <Chart
      title="Reading"
      subtitle="daily"
      data={shape.data}
      series={shape.series}
      unit="CKB"
      kind={shape.kind}
      height={360}
      hideCaption
      pointDots
      activeIndex={index}
      formatValue={(value) => formatChartHover(value, 'CKB')}
    />
  );
}

/** The reading's box and texts as drawn, each text's right end at its own size. */
function readingOf(container: HTMLElement) {
  const g = container.querySelector('[data-hover-reading]');
  if (!g) return null;
  const rect = g.querySelector('rect')!;
  const x = Number(rect.getAttribute('x'));
  return {
    rect: { left: x, right: x + Number(rect.getAttribute('width')) },
    texts: [...g.querySelectorAll('text')].map((t) => {
      const left = Number(t.getAttribute('x'));
      return {
        text: t.textContent!,
        left,
        right: left + textPx(t.textContent!.length, Number(t.getAttribute('font-size'))),
      };
    }),
  };
}

const EPS = 1e-6;
function expectWhole(container: HTMLElement, width: number, label: string) {
  const reading = readingOf(container);
  expect(reading, label).not.toBeNull();
  const { rect, texts } = reading!;
  expect(rect.left, `${label}: rect left`).toBeGreaterThanOrEqual(0);
  expect(rect.right, `${label}: rect right`).toBeLessThanOrEqual(width + EPS);
  for (const t of texts) {
    expect(t.left, `${label}: "${t.text}" left in its rect`).toBeGreaterThanOrEqual(rect.left);
    expect(t.right, `${label}: "${t.text}" right in its rect`).toBeLessThanOrEqual(
      rect.right + EPS
    );
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
          if (needed + 1 <= width) expectWhole(container, width, label);
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
      expectWhole(container, 1232, `1232 px, point ${index}`);
      const cx = Number(container.querySelector('[data-hover-dot]')!.getAttribute('cx'));
      const { rect } = readingOf(container)!;
      if (index < 29) expect(rect.left, `point ${index}`).toBeGreaterThan(cx);
      else expect(rect.right, `point ${index}`).toBeLessThan(cx);
      unmount();
    }
  });
});
