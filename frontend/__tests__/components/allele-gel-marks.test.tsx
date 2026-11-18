import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { AlleleGel, type GenotypeOverlay } from '@/components/ui/allele-gel';
import { ALLELE_GEL, alleleGelLayout } from '@/lib/allele-gel-layout';
import type { AlleleLane } from '@/lib/dob-genes';
import {
  allTextLines,
  expectApart,
  expectInside,
  rectBox,
  textLines,
  viewBox,
} from '@/__tests__/utils/svg-text-boxes';

// PB-04 (UI review 2026-11-14): "this one" on the object page's allele gel was a line drawn through
// the middle of its own band's name ("COPPER", "Bitcoin inscr…" read as struck out), and on a
// phone the lanes started 62 px in even where no tag needed that margin (the cluster page).

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const band = (label: string, count: number, total: number) => ({
  value: label,
  label,
  count,
  share: count / total,
  isColour: false,
  colour: null,
});
const lane = (gene: string, labels: [string, number][]): AlleleLane => {
  const total = labels.reduce((s, [, n]) => s + n, 0);
  return {
    gene,
    bands: labels.map(([l, n]) => band(l, n, total)),
    variants: labels.length,
    conserved: false,
    ordered: true,
  };
};
const LANES = [
  lane('Level', [
    ['COPPER', 40],
    ['SILVER', 30],
    ['GOLD', 20],
  ]),
  lane('Background', [
    ['Bitcoin inscription', 50],
    ['Nature Green', 30],
    ['Thai Spice', 10],
  ]),
];
const THIS_ONE: GenotypeOverlay = {
  key: 'me',
  label: 'this one',
  points: [
    { lane: 0, band: 0 },
    { lane: 1, band: 0 },
  ],
  persistent: true,
};

interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}
const num = (el: Element, a: string) => Number(el.getAttribute(a));
/** A band name's box: 10 px mono, centred on its x, baseline 3.5 px under the band's middle. */
function labelBox(text: Element): Box {
  const w = text.textContent!.length * ALLELE_GEL.charWidth;
  const x = num(text, 'x');
  const baseline = num(text, 'y');
  return { x0: x - w / 2, x1: x + w / 2, y0: baseline - 7.5, y1: baseline + 2.5 };
}
const overlaps = (a: Box, b: Box) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1;
/** Every stroke of the "this one" mark as a thin box (a rect's four edges, a line's span). */
function markStrokes(overlay: Element): Box[] {
  const strokes: Box[] = [];
  for (const r of overlay.querySelectorAll('rect')) {
    const [x, y, w, h] = ['x', 'y', 'width', 'height'].map((a) => num(r, a));
    strokes.push(
      { x0: x, x1: x + w, y0: y - 0.5, y1: y + 0.5 },
      { x0: x, x1: x + w, y0: y + h - 0.5, y1: y + h + 0.5 },
      { x0: x - 0.5, x1: x + 0.5, y0: y, y1: y + h },
      { x0: x + w - 0.5, x1: x + w + 0.5, y0: y, y1: y + h }
    );
  }
  for (const l of overlay.querySelectorAll('line')) {
    const [x1, y1, x2, y2] = ['x1', 'y1', 'x2', 'y2'].map((a) => num(l, a));
    // a straight segment, sampled every px
    const n = Math.max(1, Math.ceil(Math.hypot(x2 - x1, y2 - y1)));
    for (let i = 0; i <= n; i++) {
      const x = x1 + ((x2 - x1) * i) / n;
      const y = y1 + ((y2 - y1) * i) / n;
      strokes.push({ x0: x - 0.5, x1: x + 0.5, y0: y - 0.5, y1: y + 0.5 });
    }
  }
  for (const p of overlay.querySelectorAll('polyline')) {
    const pts = p
      .getAttribute('points')!
      .split(' ')
      .map((xy) => xy.split(',').map(Number));
    for (let k = 1; k < pts.length; k++) {
      const [[x1, y1], [x2, y2]] = [pts[k - 1], pts[k]];
      const n = Math.max(1, Math.ceil(Math.hypot(x2 - x1, y2 - y1)));
      for (let i = 0; i <= n; i++) {
        const x = x1 + ((x2 - x1) * i) / n;
        const y = y1 + ((y2 - y1) * i) / n;
        strokes.push({ x0: x - 0.5, x1: x + 0.5, y0: y - 0.5, y1: y + 0.5 });
      }
    }
  }
  return strokes;
}

describe.each([1248, 358])('the "this one" mark at %i px (PB-04)', (width) => {
  it('never crosses a band name', () => {
    stubChartWidth(width);
    const { container } = render(
      <AlleleGel lanes={LANES} total={100} subject="Nervape" overlays={[THIS_ONE]} />
    );
    const names = Array.from(container.querySelectorAll('svg text')).filter((t) =>
      ['COPPER', 'Bitcoin'].some((w) => t.textContent!.startsWith(w))
    );
    expect(names).toHaveLength(2);
    const overlay = container.querySelector('[data-overlay="me"]')!;
    const strokes = markStrokes(overlay);
    expect(strokes.length).toBeGreaterThan(0);
    for (const name of names) {
      const box = labelBox(name);
      expect(
        strokes.filter((s) => overlaps(s, box)),
        `${name.textContent} is crossed`
      ).toEqual([]);
    }
  });
});

describe('the margin for a genotype tag (PB-04)', () => {
  it('gives the lanes the full width when no tag is always shown', () => {
    const layout = alleleGelLayout({ genes: ['Level', 'Background'], width: 358, subject: 'x' });
    if (layout.form !== 'gel') throw new Error('expected a gel');
    expect(layout.gutter).toBe(0);
    expect(layout.gutter + layout.rows[0].length * layout.laneWidth).toBe(358 - ALLELE_GEL.right);
  });

  it('keeps the margin for "this one", which is always shown', () => {
    const layout = alleleGelLayout({
      genes: ['Level', 'Background'],
      width: 358,
      subject: 'x',
      overlayLabels: ['this one'],
    });
    if (layout.form !== 'gel') throw new Error('expected a gel');
    expect(layout.gutter).toBeGreaterThanOrEqual(8 * ALLELE_GEL.charWidth + 4);
  });

  it('starts the lanes at the left edge on a cluster page, whose hover genotypes have no tag', () => {
    stubChartWidth(358);
    const hover: GenotypeOverlay = { ...THIS_ONE, key: '0xabc', label: '#1234', persistent: false };
    const { container } = render(
      <AlleleGel lanes={LANES} total={100} subject="Nervape" overlays={[hover]} />
    );
    const first = container.querySelector('[data-lane="Level"] rect')!;
    // the lane's hit box starts 1 px into its slot
    expect(num(first, 'x')).toBe(1);
  });
});

// Readability plan 2026-11-18, lane F3 (the figures' text budgets): at the 12 px floor every word
// of the gel stands whole in its place — a gene's caption in its lane's head (across, or turned on
// end inside its lane), a variant's name inside its band, the count of variants under its lane,
// the genotype's tag in the margin — touches no other word, and stays inside the svg, measured by
// the type modules (0.6 em a glyph; a tight line over its baseline, its descent under it). The
// real cases: the object page's two genes with "this one", and a DOB of 16 and of 24 genes (their
// captions turned on end), at the object column at 1280 (1232), at 640 (608), on phones (358, 326).
describe('every word of the gel in its place (readability F3)', () => {
  const genes = (n: number) =>
    Array.from({ length: n }, (_, i) =>
      lane(`prev.gene${String(i).padStart(2, '0')}`, [
        [`trait ${i} first`, 50],
        ['second', 30],
        ['third', 20],
      ])
    );
  // a gene whose rare variants are bands 14 to 19 px tall in the 250 px gel, each named
  const rare = lane('Eyes', [
    ['common', 76],
    ['rare', 6],
    ['rarer', 7],
    ['rarest', 8],
    ['the last', 3],
  ]);
  const cases = [
    ['two genes and "this one"', LANES, [THIS_ONE]],
    ['three genes, one with rare variants', [...LANES, rare], []],
    ['16 genes', genes(16), []],
    ['24 genes', genes(24), []],
  ] as const;
  it.each([1232, 608, 358, 326].flatMap((w) => cases.map(([n, l, o]) => [w, n, l, o] as const)))(
    'at %i px, %s',
    (width, _name, lanes, overlays) => {
      stubChartWidth(width);
      const { container } = render(
        <AlleleGel lanes={lanes} total={100} subject="Nervape" overlays={overlays} />
      );
      const svg = container.querySelector('svg')!;
      const lines = allTextLines(svg);
      for (const line of lines) expectInside(line, viewBox(svg), 'the gel', 'flush');
      expectApart(lines, `the gel at ${width}`);
      for (const g of [...svg.querySelectorAll('[data-lane]')]) {
        const lane = rectBox(g.querySelector('rect')!);
        // a caption, across or on end, stands inside its lane's width
        for (const line of textLines(g.querySelector('[data-gene-caption]')!))
          expectInside(line, { ...lane, top: -Infinity, bottom: Infinity }, 'a caption', 'flush');
        // a variant's name inside its band
        for (const band of [...g.querySelectorAll('rect[data-allele]')]) {
          const text = band.parentElement!.querySelector('text');
          if (text)
            for (const line of textLines(text))
              expectInside(line, rectBox(band), `"${line.text}" in its band`);
        }
      }
    }
  );
});
