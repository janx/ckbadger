import { describe, expect, it } from 'vitest';
import {
  alpha,
  color,
  contrastRatio,
  fade,
  glowAlpha,
  hexAlpha,
  hexToRgb,
  material,
  MATERIAL_KEYS,
  MATERIALS,
  PALETTE,
  PALETTE_KEYS,
  relativeLuminance,
  THEMES,
  themeBaseStyles,
  themeDeclarations,
  twColor,
  type PaletteKey,
} from '@/lib/palette';

const HEX = /^#[0-9a-f]{6}$/;

/** The grounds text is read on, in both themes. */
const GROUNDS: PaletteKey[] = ['base-void', 'base-bg', 'base-surface', 'base-elevated'];

/** Inks and accents that carry text: ≥ 4.5:1 on every ground (WCAG AA, body text). */
const TEXT_GRADE: PaletteKey[] = [
  'text-bright',
  'text',
  'text-dim',
  'jade',
  'jade-dim',
  'aqua',
  'aqua-dim',
  'gold',
  'gold-dim',
  'rouge',
  'lavender',
  'lavender-dim',
  'amber',
  'token',
  'object',
  'identity',
  'copper',
  'cell-lock',
  'cell-type',
  'cell-data',
  'data-framing',
  'board-link',
  'tier-btc-ink',
  'tier-pure-ink',
];

/** Hover and secondary inks: ≥ 3:1 (large text, UI components). */
const UI_GRADE: PaletteKey[] = [
  'ink-muted',
  // the cinnabar text dimmed: a secondary ink, never body text
  'rouge-dim',
  'jade-bright',
  'aqua-bright',
  'gold-bright',
  'rouge-bright',
  'token-bright',
  'amber-dim',
  'flow-ckb',
  // cinnabar itself: prompt squares, seals, the search square (text in cinnabar is `rouge`)
  'mark',
];

// The hex below are values, not colours a page draws: the palette is the one place they live.
// RUBBING is the night theme as the design boards set it (docs/superpowers/design/
// 2026-11-10-rubbing-song/src/lib.js THEMES.dark), dim lightened from #8a8073 to body contrast.
const RUBBING = {
  jade: '#93c1b6',
  jadeDim: '#6f9a90',
  jadeBright: '#b7d9d0',
  aqua: '#7fa3c9',
  gold: '#c9a45c',
  rouge: '#d9634a',
  lavender: '#a797c4',
  amber: '#a3bf7a',
  mark: '#c2452d',
  textBright: '#e8e2d6',
  text: '#b9b0a2',
  textDim: '#908679',
  textGhost: '#4a4139',
  quiet: '#60574d',
  void: '#0f0d0b',
  bg: '#15120f',
  surface: '#1b1714',
  elevated: '#241f1a',
  border: '#2a231d',
  borderSubtle: '#221c17',
  copper: '#cc8a5e',
  token: '#6f9ad6',
  object: '#a58ad6',
  identity: '#c97f93',
};

/** The alpha of an `rgba(r, g, b, a)` material. */
function rgbaAlpha(value: string): number {
  const m = value.match(/^rgba\(\d+, \d+, \d+, ([\d.]+)\)$/);
  if (!m) throw new Error(`${value} is not rgba(r, g, b, a)`);
  return Number(m[1]);
}

describe('the palette: one source for both themes', () => {
  it('defines every colour key in both themes as #rrggbb', () => {
    for (const theme of THEMES) {
      expect(Object.keys(PALETTE[theme]).sort()).toEqual([...PALETTE_KEYS].sort());
      for (const key of PALETTE_KEYS) expect(PALETTE[theme][key], `${theme} ${key}`).toMatch(HEX);
    }
    expect(PALETTE_KEYS.length).toBeGreaterThan(100);
  });

  it('defines every material in both themes', () => {
    for (const theme of THEMES)
      expect(Object.keys(MATERIALS[theme]).sort()).toEqual([...MATERIAL_KEYS].sort());
  });

  it('keeps the night theme at the rubbing values the design boards set', () => {
    const { dark } = PALETTE;
    expect(dark.jade).toBe(RUBBING.jade);
    expect(dark['jade-dim']).toBe(RUBBING.jadeDim);
    expect(dark['jade-bright']).toBe(RUBBING.jadeBright);
    expect(dark.aqua).toBe(RUBBING.aqua);
    expect(dark.gold).toBe(RUBBING.gold);
    expect(dark.rouge).toBe(RUBBING.rouge);
    expect(dark.lavender).toBe(RUBBING.lavender);
    expect(dark.amber).toBe(RUBBING.amber);
    expect(dark.mark).toBe(RUBBING.mark);
    expect(dark['text-bright']).toBe(RUBBING.textBright);
    expect(dark.text).toBe(RUBBING.text);
    expect(dark['text-dim']).toBe(RUBBING.textDim);
    expect(dark['text-ghost']).toBe(RUBBING.textGhost);
    expect(dark['ink-quiet']).toBe(RUBBING.quiet);
    expect(dark['base-void']).toBe(RUBBING.void);
    expect(dark['base-bg']).toBe(RUBBING.bg);
    expect(dark['base-surface']).toBe(RUBBING.surface);
    expect(dark['base-elevated']).toBe(RUBBING.elevated);
    expect(dark['base-border']).toBe(RUBBING.border);
    expect(dark['base-border-subtle']).toBe(RUBBING.borderSubtle);
    expect(dark.copper).toBe(RUBBING.copper);
    expect(dark.token).toBe(RUBBING.token);
    expect(dark.object).toBe(RUBBING.object);
    expect(dark.identity).toBe(RUBBING.identity);
    expect(MATERIALS.dark.glow).toBe('1');
  });

  it('keeps the mark, cinnabar, at UI contrast on every ground in both themes', () => {
    for (const theme of THEMES)
      for (const ground of GROUNDS) {
        const ratio = contrastRatio(PALETTE[theme].mark, PALETTE[theme][ground]);
        expect(ratio, `${theme} mark on ${ground}`).toBeGreaterThanOrEqual(3);
      }
    // the Song print mark is the boards' day cinnabar; the mark is not the text ink
    expect(PALETTE.light.mark).toBe('#b5331f');
    expect(PALETTE.dark.mark).not.toBe(PALETTE.dark.rouge);
    expect(PALETTE.light.mark).not.toBe(PALETTE.light.rouge);
  });

  it('is dark ink on paper by day and light ink on night by night', () => {
    const { dark, light } = PALETTE;
    expect(relativeLuminance(light['base-bg'])).toBeGreaterThan(0.8);
    expect(relativeLuminance(light['text-bright'])).toBeLessThan(0.02);
    expect(relativeLuminance(dark['base-bg'])).toBeLessThan(0.01);
    expect(relativeLuminance(dark['text-bright'])).toBeGreaterThan(0.7);
    // the grounds step: paper, then a whiter sheet for a surface, then a darker tint for a hover
    expect(relativeLuminance(light['base-surface'])).toBeGreaterThan(
      relativeLuminance(light['base-bg'])
    );
    expect(relativeLuminance(light['base-elevated'])).toBeLessThan(
      relativeLuminance(light['base-bg'])
    );
    // and nothing glows on paper
    expect(MATERIALS.light.glow).toBe('0');
  });

  it('reads at body-text contrast on every ground, in both themes', () => {
    const failures: string[] = [];
    for (const theme of THEMES)
      for (const ink of TEXT_GRADE)
        for (const ground of GROUNDS) {
          const ratio = contrastRatio(PALETTE[theme][ink], PALETTE[theme][ground]);
          if (ratio < 4.5) failures.push(`${theme} ${ink} on ${ground}: ${ratio.toFixed(2)}`);
        }
    expect(failures).toEqual([]);
  });

  it('keeps hover and secondary inks at UI contrast on every ground', () => {
    const failures: string[] = [];
    for (const theme of THEMES)
      for (const ink of UI_GRADE)
        for (const ground of GROUNDS) {
          const ratio = contrastRatio(PALETTE[theme][ink], PALETTE[theme][ground]);
          if (ratio < 3) failures.push(`${theme} ${ink} on ${ground}: ${ratio.toFixed(2)}`);
        }
    expect(failures).toEqual([]);
  });

  it('keeps the jade ramp ordered, strongest first, in both themes', () => {
    for (const theme of THEMES) {
      const ramp = ['jade', 'ramp-1', 'ramp-2', 'ramp-3', 'ramp-4', 'ramp-5'] as const;
      const ground = PALETTE[theme]['base-bg'];
      const contrasts = ramp.map((k) => contrastRatio(PALETTE[theme][k], ground));
      for (let i = 1; i < contrasts.length; i++)
        expect(contrasts[i], `${theme} ${ramp[i]}`).toBeLessThan(contrasts[i - 1]);
    }
  });
});

describe('the rubbing / Song print finish', () => {
  it('draws CRT scanlines by night and ruled paper by day: the same line, the day one fainter', () => {
    const night = MATERIALS.dark['scanline-ink'];
    const day = MATERIALS.light['scanline-ink'];
    expect(night).not.toBe(day);
    // the boards' `scan`: black at 22 % by night, the day ink (#1f1b17) at 4.5 % by day
    expect(night).toBe('rgba(0, 0, 0, 0.22)');
    expect(day).toBe(hexAlpha(PALETTE.light['text-bright'], 0.045));
    expect(rgbaAlpha(day)).toBeLessThan(rgbaAlpha(night));
  });

  it('sets a frame flat: the surface, opaque, with no sheen, no lit edges and no outer ring', () => {
    for (const theme of THEMES) {
      const m = MATERIALS[theme];
      expect(m['glass-fill'], theme).toBe(hexAlpha(PALETTE[theme]['base-surface'], 1));
      for (const key of [
        'glass-sheen',
        'glass-top-edge',
        'glass-bottom-edge',
        'glass-ring',
      ] as const)
        expect(m[key], `${theme} ${key}`).toBe('transparent');
      // a well is the boards' inset, without the lip
      expect(m['well-lip'], theme).toBe('transparent');
    }
    expect(MATERIALS.dark['well-inset']).toBe('rgba(0, 0, 0, 0.6)');
    expect(MATERIALS.light['well-inset']).toBe(hexAlpha(PALETTE.light['text-bright'], 0.08));
  });

  it('softens the vignette and the grain to about half their midnight strength', () => {
    // midnight: a 35 % vignette and 3 % grain by night, 6 % and 5 % by day
    expect(rgbaAlpha(MATERIALS.dark.vignette)).toBeLessThanOrEqual(0.35 / 2 + 0.01);
    expect(rgbaAlpha(MATERIALS.light.vignette)).toBeLessThanOrEqual(0.06 / 2 + 0.005);
    expect(Number(MATERIALS.dark['grain-opacity'])).toBeLessThan(0.03);
    expect(Number(MATERIALS.light['grain-opacity'])).toBeLessThan(0.05);
  });

  it('keeps the sticky vitals strip opaque: the page scrolls under it, never through it', () => {
    // midnight let the page show through an 80 % strip; the terminal skeleton has no translucency
    for (const theme of THEMES) {
      expect(MATERIALS[theme]['header-strip'], theme).toBe(PALETTE[theme]['base-surface']);
      expect(MATERIALS[theme]['header-strip'], theme).toMatch(/^#[0-9a-f]{6}$/);
    }
  });
});

describe('the palette as CSS', () => {
  it('declares a colour, its channels and the materials per theme', () => {
    const dark = themeDeclarations('dark');
    expect(dark['--c-jade']).toBe(RUBBING.jade);
    expect(dark['--rgb-jade']).toBe('147 193 182');
    expect(dark['--c-mark']).toBe(RUBBING.mark);
    expect(dark['--m-glow']).toBe('1');
    const light = themeDeclarations('light');
    expect(light['--c-jade']).toBe(PALETTE.light.jade);
    expect(light['--m-glow']).toBe('0');
    expect(Object.keys(light)).toEqual(Object.keys(dark));
  });

  it('puts the dark theme on :root and the light theme behind the data-theme attribute', () => {
    const styles = themeBaseStyles();
    expect(Object.keys(styles)).toEqual([
      ":root, :root[data-theme='dark']",
      ":root[data-theme='light']",
    ]);
    expect(styles[":root, :root[data-theme='dark']"]['--c-base-bg']).toBe(RUBBING.bg);
    expect(styles[":root[data-theme='light']"]['--c-base-bg']).toBe(PALETTE.light['base-bg']);
  });

  it('references a colour, an alpha, a glow and a material by variable', () => {
    expect(color('jade')).toBe('var(--c-jade)');
    expect(alpha('jade', 0.25)).toBe('rgb(var(--rgb-jade) / 0.25)');
    expect(glowAlpha('jade', 0.45)).toBe('rgb(var(--rgb-jade) / calc(0.45 * var(--m-glow)))');
    expect(material('glass-fill')).toBe('var(--m-glass-fill)');
    expect(twColor('base-bg')).toBe('rgb(var(--rgb-base-bg) / <alpha-value>)');
    expect(() => alpha('jade', 1.5)).toThrow('alpha 1.5 of jade');
    expect(() => glowAlpha('jade', -1)).toThrow('alpha -1 of jade');
  });

  it('fades any colour reference toward transparent, and a hex into rgba for a canvas', () => {
    expect(fade(color('jade'), 0.15)).toBe('color-mix(in srgb, var(--c-jade) 15%, transparent)');
    expect(fade(RUBBING.jade, 0.125)).toBe(
      `color-mix(in srgb, ${RUBBING.jade} 12.5%, transparent)`
    );
    expect(() => fade('', 0.5)).toThrow('empty colour');
    expect(() => fade(color('jade'), 2)).toThrow('not in 0..1');
    expect(hexAlpha(RUBBING.jade, 0.2)).toBe('rgba(147, 193, 182, 0.2)');
    expect(() => hexAlpha(color('jade'), 0.2)).toThrow('not #rrggbb');
  });

  it('parses hex and refuses what is not #rrggbb', () => {
    expect(hexToRgb(RUBBING.jade)).toEqual({ r: 147, g: 193, b: 182 });
    expect(() => hexToRgb('#fff')).toThrow('#fff is not #rrggbb');
    expect(() => hexToRgb('var(--c-jade)')).toThrow('not #rrggbb');
    // white on black is 21:1, the scale's end
    expect(contrastRatio('#ffffff', '#000000')).toBeCloseTo(21, 5);
  });
});
