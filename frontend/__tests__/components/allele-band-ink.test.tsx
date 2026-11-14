import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { AlleleGel, type AlleleGelProps } from '@/components/ui/allele-gel';
import { alleleLanes, geneOrder } from '@/lib/dob-genes';
import { ALLELE_GEL } from '@/lib/allele-gel-layout';
import { nervape3xInsights } from '@/__tests__/fixtures/objects/nervape3x';
import {
  FILL_INKS,
  PALETTE,
  PALETTE_KEYS,
  contrastRatio,
  hexToRgb,
  overGround,
  type PaletteKey,
  type ThemeName,
} from '@/lib/palette';
import { resetThemeForTests } from '@/lib/theme';

// Lane E2 round 4 (ruling): the allele gel's band labels are words on a fill. They were set in
// `gel-band-text` by hand (1.83–3.04:1 by night and 1.36–1.78:1 by day at the lowest band opacity,
// 2.80:1 by day at full opacity on gel-ink-btc-0). Each label takes the one ink the band's fill
// calls for at its opacity over the ground the gel is drawn on, in the theme in force (inkOnFill's
// rule: of the well and the bright ink, the one that reads stronger), so it reads at 4.5:1 or more
// wherever either ink can. The band's fill is a gradient across the plate, so the label is read on
// every colour under it (its two ends and any stop between). Where neither ink reaches 4.5:1 the
// band is listed with the best ratio there is, and left as the ruling says: no ink invented, no
// palette value moved.

const lanes = alleleLanes(
  nervape3xInsights.genetics,
  geneOrder(nervape3xInsights.currentPattern),
  'Nervape 3x'
);
const isKey = (key: string): key is PaletteKey => (PALETTE_KEYS as string[]).includes(key);
const keyOf = (ref: string | null): PaletteKey => {
  const key = /^var\(--c-([a-z0-9-]+)\)$/.exec(ref ?? '')?.[1];
  if (!key || !isKey(key)) throw new Error(`${ref} is not a palette colour`);
  return key;
};
/** `b` at `t` of the way from `a` (two hexes, sRGB, as an SVG gradient interpolates). */
function lerp(a: string, b: string, t: number): string {
  const [x, y] = [hexToRgb(a), hexToRgb(b)];
  const c = (p: number, q: number) =>
    Math.round(p + (q - p) * t)
      .toString(16)
      .padStart(2, '0');
  return `#${c(x.r, y.r)}${c(x.g, y.g)}${c(x.b, y.b)}`;
}

function inTheme(theme: ThemeName) {
  localStorage.setItem('ckbadger.theme', theme);
  resetThemeForTests();
  document.documentElement.setAttribute('data-theme', theme);
}

afterEach(() => {
  localStorage.clear();
  resetThemeForTests();
  document.documentElement.setAttribute('data-theme', 'dark');
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

interface Band {
  label: string;
  alpha: number;
  ink: PaletteKey;
  /** The best of the two fill inks on the colours under the label, and its ratio. */
  best: { ink: PaletteKey; ratio: number };
  /** The label's own ink's lowest ratio on those colours. */
  ratio: number;
}

/** Every labelled band of a rendered gel: its label's ink and what its fill calls for. */
function labelledBands(container: HTMLElement, theme: ThemeName, ground: PaletteKey): Band[] {
  const svg = container.querySelector('svg[role="img"]') as SVGSVGElement;
  const gradient = svg.querySelector('linearGradient') as SVGLinearGradientElement;
  const width = Number(gradient.getAttribute('x2'));
  const stops = [...gradient.querySelectorAll('stop')].map((s) => ({
    at: Number(s.getAttribute('offset')) * width,
    key: keyOf(s.getAttribute('stop-color')),
  }));
  const bands: Band[] = [];
  for (const rect of svg.querySelectorAll('rect[data-allele]')) {
    const text = rect.nextElementSibling;
    if (!text || text.tagName.toLowerCase() !== 'text') continue;
    const alpha = Number(rect.getAttribute('fill-opacity'));
    const label = text.textContent ?? '';
    const cx = Number(text.getAttribute('x'));
    const half = (label.length * ALLELE_GEL.charWidth) / 2;
    // the composite of the gradient over the ground, at x
    const at = (x: number) => {
      const i = stops.findIndex((s, k) => k + 1 < stops.length && x <= stops[k + 1].at);
      const [s0, s1] = [stops[i], stops[i + 1]];
      const t = Math.min(1, Math.max(0, (x - s0.at) / (s1.at - s0.at)));
      return lerp(
        overGround(theme, s0.key, alpha, ground),
        overGround(theme, s1.key, alpha, ground),
        t
      );
    };
    const xs = [cx - half, cx + half, ...stops.map((s) => s.at)].filter(
      (x) => x >= cx - half && x <= cx + half
    );
    const under = xs.map(at);
    const worst = (ink: PaletteKey) =>
      Math.min(...under.map((c) => contrastRatio(PALETTE[theme][ink], c)));
    const best = FILL_INKS.map((ink) => ({ ink: ink as PaletteKey, ratio: worst(ink) })).reduce(
      (a, b) => (b.ratio > a.ratio ? b : a)
    );
    const ink = keyOf(text.getAttribute('fill'));
    bands.push({ label, alpha, ink, best, ratio: worst(ink) });
  }
  return bands;
}

/**
 * The gel's props for a plate: the fixture's lanes in `ink`, on `ground` (the colour the plate is,
 * which the gel needs to choose its labels' ink). Written as an assertion until the gel takes it.
 */
const gelProps = (ink: AlleleGelProps['ink'], ground: PaletteKey) =>
  ({ lanes, total: 90, subject: 'Nervape 3x', ink, ground }) as AlleleGelProps;

// Where the gel is drawn: the object page's plate (the instrument frame on the surface, a BTC+CKB
// or pure-CKB object's copper plate on its inner fill) and the collection page (the page ground).
const PLATES = [
  ['other', 'base-surface'],
  ['btc_ckb', 'badge-inner-btc'],
  ['pure_ckb', 'badge-inner-pure'],
  ['other', 'base-bg'],
  ['btc_ckb', 'base-bg'],
  ['pure_ckb', 'base-bg'],
] as const;

describe.each(['dark', 'light'] as const)(
  'the allele gel’s band labels in the %s theme',
  (theme) => {
    it.each(PLATES)(
      'labels every band of a %s gel on %s in the ink its fill calls for, at 4.5:1 where any ink can',
      (ink, ground) => {
        inTheme(theme);
        stubChartWidth(900);
        const { container } = render(<AlleleGel {...gelProps(ink, ground)} />);
        const bands = labelledBands(container, theme, ground);
        expect(bands.length).toBeGreaterThan(3);
        const wrong = bands
          .filter((b) => b.ink !== b.best.ink || (b.best.ratio >= 4.5 && b.ratio < 4.5))
          .map(
            (b) =>
              `"${b.label}" at ${b.alpha}: ${b.ink} ${b.ratio.toFixed(2)}, its fill calls for ${b.best.ink} ${b.best.ratio.toFixed(2)}`
          );
        expect(wrong).toEqual([]);
      }
    );
  }
);

// The bands where neither ink reaches 4.5:1 (the fill's luminance sits between the two inks'
// reach): stopped on, as the ruling says, each with the best ratio there is.
describe('the bands no fill ink can label at 4.5:1', () => {
  it('are these, and only these', () => {
    const stopped: string[] = [];
    for (const theme of ['dark', 'light'] as const)
      for (const [ink, ground] of PLATES) {
        inTheme(theme);
        stubChartWidth(900);
        const { container, unmount } = render(<AlleleGel {...gelProps(ink, ground)} />);
        for (const b of labelledBands(container, theme, ground))
          if (b.best.ratio < 4.5)
            stopped.push(
              `${theme} ${ink} on ${ground}: "${b.label}" at ${b.alpha}, best ${b.best.ink} ${b.best.ratio.toFixed(2)}`
            );
        unmount();
      }
    expect(stopped).toEqual(STOPPED);
  });
});

const STOPPED: string[] = [
  'dark other on base-surface: "Blush Pink" at 0.68, best text-bright 3.94',
  'dark other on base-surface: "Nature Green" at 0.68, best text-bright 3.94',
  'dark other on base-surface: "Thai Special" at 0.68, best text-bright 3.94',
  'dark btc_ckb on badge-inner-btc: "Blush Pink" at 0.68, best base-void 4.17',
  'dark btc_ckb on badge-inner-btc: "Nature Green" at 0.68, best base-void 4.12',
  'dark btc_ckb on badge-inner-btc: "Thai Special" at 0.68, best base-void 4.12',
  'dark pure_ckb on badge-inner-pure: "Lucky Lavender" at 0.51, best base-void 4.07',
  'dark pure_ckb on badge-inner-pure: "Pure White" at 0.51, best base-void 4.13',
  'dark other on base-bg: "Blush Pink" at 0.68, best text-bright 4.02',
  'dark other on base-bg: "Nature Green" at 0.68, best text-bright 4.02',
  'dark other on base-bg: "Thai Special" at 0.68, best text-bright 4.02',
  'dark btc_ckb on base-bg: "Blush Pink" at 0.68, best base-void 4.09',
  'dark btc_ckb on base-bg: "Nature Green" at 0.68, best base-void 4.05',
  'dark btc_ckb on base-bg: "Thai Special" at 0.68, best base-void 4.05',
  'dark pure_ckb on base-bg: "Lucky Lavender" at 0.51, best base-void 3.87',
  'dark pure_ckb on base-bg: "Pure White" at 0.51, best base-void 3.92',
  'light btc_ckb on badge-inner-btc: "Calm Blue" at 0.84, best base-void 4.32',
  'light btc_ckb on badge-inner-btc: "Fortune Gold" at 0.84, best base-void 4.32',
  'light pure_ckb on badge-inner-pure: "Fortune Gold" at 0.84, best base-void 4.46',
  'light btc_ckb on base-bg: "Calm Blue" at 0.84, best base-void 4.27',
  'light btc_ckb on base-bg: "Fortune Gold" at 0.84, best base-void 4.28',
  'light pure_ckb on base-bg: "Calm Blue" at 0.84, best base-void 4.45',
  'light pure_ckb on base-bg: "Fortune Gold" at 0.84, best base-void 4.39',
];
