import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { AlleleGel, type GenotypeOverlay } from '@/components/ui/allele-gel';
import { ALLELE_GEL, alleleGelLayout } from '@/lib/allele-gel-layout';
import type { AlleleLane } from '@/lib/dob-genes';

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
