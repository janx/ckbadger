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
  // the designed midnight rouge-dim sits at 3.6:1; a secondary ink since before the light theme
  'rouge-dim',
  'jade-bright',
  'aqua-bright',
  'gold-bright',
  'rouge-bright',
  'token-bright',
  'amber-dim',
  'flow-ckb',
];

// The hex below are values, not colours a page draws: the palette is the one place they live.
const MIDNIGHT = {
  jade: '#2edba3',
  aqua: '#68ccf0',
  gold: '#f2c55c',
  rouge: '#e8555a',
  textBright: '#dee2ec',
  text: '#a0a8be',
  textDim: '#7c849c',
  textGhost: '#343c50',
  quiet: '#4b5266',
  bg: '#0c0e15',
  surface: '#10131c',
  border: '#222840',
  borderSubtle: '#1a1f30',
  copper: '#dc8a5a',
  token: '#5aa6f5',
  object: '#a98cf5',
  identity: '#f08aa8',
};

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

  it('keeps the midnight theme at the values the site was designed in', () => {
    const { dark } = PALETTE;
    expect(dark.jade).toBe(MIDNIGHT.jade);
    expect(dark.aqua).toBe(MIDNIGHT.aqua);
    expect(dark.gold).toBe(MIDNIGHT.gold);
    expect(dark.rouge).toBe(MIDNIGHT.rouge);
    expect(dark['text-bright']).toBe(MIDNIGHT.textBright);
    expect(dark.text).toBe(MIDNIGHT.text);
    expect(dark['text-dim']).toBe(MIDNIGHT.textDim);
    expect(dark['text-ghost']).toBe(MIDNIGHT.textGhost);
    expect(dark['ink-quiet']).toBe(MIDNIGHT.quiet);
    expect(dark['base-bg']).toBe(MIDNIGHT.bg);
    expect(dark['base-surface']).toBe(MIDNIGHT.surface);
    expect(dark['base-border']).toBe(MIDNIGHT.border);
    expect(dark['base-border-subtle']).toBe(MIDNIGHT.borderSubtle);
    expect(dark.copper).toBe(MIDNIGHT.copper);
    expect(dark.token).toBe(MIDNIGHT.token);
    expect(dark.object).toBe(MIDNIGHT.object);
    expect(dark.identity).toBe(MIDNIGHT.identity);
    expect(MATERIALS.dark.glow).toBe('1');
  });

  it('is dark ink on paper by day and light ink on night by night', () => {
    const { dark, light } = PALETTE;
    expect(relativeLuminance(light['base-bg'])).toBeGreaterThan(0.8);
    expect(relativeLuminance(light['text-bright'])).toBeLessThan(0.02);
    expect(relativeLuminance(dark['base-bg'])).toBeLessThan(0.01);
    expect(relativeLuminance(dark['text-bright'])).toBeGreaterThan(0.7);
    // the grounds step: paper, then white for a surface, then a grey tint for a hover
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

describe('the palette as CSS', () => {
  it('declares a colour, its channels and the materials per theme', () => {
    const dark = themeDeclarations('dark');
    expect(dark['--c-jade']).toBe(MIDNIGHT.jade);
    expect(dark['--rgb-jade']).toBe('46 219 163');
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
    expect(styles[":root, :root[data-theme='dark']"]['--c-base-bg']).toBe(MIDNIGHT.bg);
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
    expect(fade(MIDNIGHT.jade, 0.125)).toBe(
      `color-mix(in srgb, ${MIDNIGHT.jade} 12.5%, transparent)`
    );
    expect(() => fade('', 0.5)).toThrow('empty colour');
    expect(() => fade(color('jade'), 2)).toThrow('not in 0..1');
    expect(hexAlpha(MIDNIGHT.jade, 0.2)).toBe('rgba(46, 219, 163, 0.2)');
    expect(() => hexAlpha(color('jade'), 0.2)).toThrow('not #rrggbb');
  });

  it('parses hex and refuses what is not #rrggbb', () => {
    expect(hexToRgb(MIDNIGHT.jade)).toEqual({ r: 46, g: 219, b: 163 });
    expect(() => hexToRgb('#fff')).toThrow('#fff is not #rrggbb');
    expect(() => hexToRgb('var(--c-jade)')).toThrow('not #rrggbb');
    // white on black is 21:1, the scale's end
    expect(contrastRatio('#ffffff', '#000000')).toBeCloseTo(21, 5);
  });
});
