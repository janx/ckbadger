import { describe, expect, it } from 'vitest';
import { AMOUNT_INK } from '@/components/ui/ckb-amount';
import { STEP_W } from '@/lib/pipeline/encoding';
import { CODE_CLASS_KEYS } from '@/lib/design-tokens';
import {
  alpha,
  apcaLc,
  color,
  contrastRatio,
  fade,
  FILL_SCAN,
  glowAlpha,
  hexAlpha,
  hexToRgb,
  inkOnFill,
  material,
  MATERIAL_KEYS,
  MATERIALS,
  overGround,
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
  // the cinnabar text's quiet step, like every other `*-dim` (re-pinned on purpose: it was a 3:1
  // secondary ink, and it is used as text); a quiet cinnabar word (R4, DY-06)
  'rouge-dim',
  'token',
  'object',
  'identity',
  'copper',
  'cell-lock',
  'cell-type',
  'cell-data',
  'data-framing',
  'board-link',
  // Lane E2 round 3 (ruling): an ELF region's hue paints the hex digits of its bytes on the cell
  // page, so each is text grade, also on the evidence tier's ground (below). The program headers
  // are the header a step lighter by night and a step darker by day (round 4's ruling).
  'elf-header',
  'elf-program-headers',
  'elf-rodata',
  'elf-eh-frame',
  'elf-data',
  'elf-meta',
];

/** Hover and secondary inks: ≥ 3:1 (large text, UI components). */
const UI_GRADE: PaletteKey[] = [
  // a mark's ink at 3:1 (a rule, a tick, a swatch), never a word's: see "the inks under text grade"
  'ink-muted',
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
// RUBBING is the night theme as the boards set it: the rubbing of 2026-11-10 (docs/superpowers/
// design/2026-11-10-rubbing-song/src/lib.js THEMES.dark), its inks and structure re-graded by the
// boards' ruling of 2026-11-18 (readability plan, ruling R2, Gate F0), which replaces them: the
// prototype's ladder (reading ink Lc 75, quiet 60, muted 45, quiet slate 30, ghost 20; accents and
// classes at Lc 70; rules 1.25:1, frames 1.40:1 to the page ground), moved in OKLCH lightness only
// where a pin asked for more: Lc 75 and 60 on the surface as on the page (text, text-dim, four
// -dim steps, rouge-dim and rouge kept over it, data-framing, elf-meta), the ELF program headers a
// step over their header, and text-bright for the labelled bars' 4.5:1 (fill-ink.test). The mark,
// the page ground and the well are the boards' as before.
const RUBBING = {
  jade: '#9ecdc2',
  jadeDim: '#90bdb2',
  jadeBright: '#b9dcd2',
  aqua: '#a2c8ef',
  gold: '#e4bf76',
  rouge: '#ff9e88',
  lavender: '#cdbceb',
  amber: '#b1ce88',
  mark: '#c2452d',
  textBright: '#ede7db',
  text: '#d6cdbe',
  textDim: '#bdb3a4',
  textGhost: '#685f56',
  quiet: '#7f766b',
  void: '#0f0d0b',
  bg: '#15120f',
  surface: '#1e1a17',
  elevated: '#26211c',
  border: '#362e28',
  borderSubtle: '#2d2621',
  copper: '#fab487',
  token: '#9ec6fe',
  object: '#d0b8fe',
  identity: '#fcaec2',
  // The darker code-class grey, set by its words (readability F1 follow-up): the bright ink at Lc
  // 70 on it. It was the mix dim ghost 0.4, #8a8175 since the ruling (#665d53 before), unpinned.
  codeUnspendableFill: '#6a6155',
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
    expect(dark['code-unspendable-fill']).toBe(RUBBING.codeUnspendableFill);
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

  // Lane E2 follow-up (ruling R4 is about what an ink is used for, not its name): the ghost, the
  // quiet and the muted ink sit under text grade on purpose. They are for marks, rules, ticks,
  // swatches, frames, the padding zeros of a number and pure decoration, never for a word or a
  // figure; the muted's 3:1 pin above is a mark's, not a text pin. The ink gate refuses all three
  // on words (__tests__/lib/ink-gate.test.ts); a quieter word is the dim ink.
  // Re-pinned on purpose by night (readability ruling R2): the night's text grade is perceptual,
  // so "under text grade" is measured there in Lc, under the word floor of 60 on every ground (the
  // muted ink is Lc 45 and 5.4–6.6:1, the quiet slate Lc 30: marks by the grade the night is read
  // at; the rising order and the day's side are pinned in "the text grade by perceptual
  // contrast"). By day the WCAG measure stands as it was.
  it('keeps the inks under text grade (ghost, quiet, muted) out of the text grade: marks, never words', () => {
    for (const key of ['text-ghost', 'ink-quiet', 'ink-muted'] as const) {
      expect(TEXT_GRADE).not.toContain(key);
      const worstDay = Math.min(
        ...GROUNDS.map((g) => contrastRatio(PALETTE.light[key], PALETTE.light[g]))
      );
      expect(worstDay, `light ${key}`).toBeLessThan(4.5);
      const bestNight = Math.max(
        ...GROUNDS.map((g) => Math.abs(apcaLc(PALETTE.dark[key], PALETTE.dark[g])))
      );
      expect(bestNight, `dark ${key}`).toBeLessThan(60);
    }
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
  // Re-pinned for readability R1 (Gate F0): the boards' `scan` was black at 22 % by night over
  // the whole page, seen only where it crossed the text (its strokes lost 41–43 % of their
  // luminance on the rows it hit). By night the window's scan is transparent; by day it stays the
  // ruled paper, the day ink (#1f1b17) at 4.5 %.
  it('lays ruled paper over the page by day and no scan by night', () => {
    expect(MATERIALS.dark['scanline-ink']).toBe('transparent');
    expect(MATERIALS.light['scanline-ink']).toBe(hexAlpha(PALETTE.light['text-bright'], 0.045));
    expect(rgbaAlpha(MATERIALS.light['scanline-ink'])).toBeGreaterThan(0);
  });

  // Readability R1: the night's scan is cut into a figure's fills, in the window layer's rhythm
  // and at the strength it had on a fill: of every 3 px, 1 px lets the ground through at 22 %.
  // `fill-scan` is the HTML fill's mask image (an alpha mask from the fill's foot up); by day
  // there is none, the ruled paper being the window's.
  it('cuts the scan into a fill by night, 1 px in 3 at the layer’s 22 %, and none by day', () => {
    expect(FILL_SCAN).toEqual({ period: 3, row: 1, strength: 0.22 });
    expect(MATERIALS.dark['fill-scan']).toBe(
      'repeating-linear-gradient(0deg, rgba(0, 0, 0, 1) 0px, rgba(0, 0, 0, 1) 2px, rgba(0, 0, 0, 0.78) 2px, rgba(0, 0, 0, 0.78) 3px)'
    );
    expect(MATERIALS.light['fill-scan']).toBe('none');
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

  // Phase 4: the newest block's seal is pressed behind the 404 verse in the cinnabar mark at
  // `seal-press`; the verse's three inks, the dim facts and the links stay text-grade on a stroke.
  it('presses the 404 seal faint: every ink of the verse and its facts is text-grade on a stroke', () => {
    for (const theme of THEMES) {
      const press = Number(MATERIALS[theme]['seal-press']);
      expect(press, theme).toBeGreaterThan(0);
      expect(press, theme).toBeLessThanOrEqual(0.2);
      const ground = hexToRgb(PALETTE[theme]['base-bg']);
      const ink = hexToRgb(PALETTE[theme].mark);
      const channel = (k: 'r' | 'g' | 'b') =>
        Math.round(ground[k] * (1 - press) + ink[k] * press)
          .toString(16)
          .padStart(2, '0');
      const stroke = `#${channel('r')}${channel('g')}${channel('b')}`;
      for (const key of ['text-bright', 'text', 'text-dim', 'jade'] as const)
        expect(
          contrastRatio(PALETTE[theme][key], stroke),
          `${theme} ${key}`
        ).toBeGreaterThanOrEqual(4.5);
    }
    // paper takes the same red more strongly: the day press is the fainter
    expect(Number(MATERIALS.light['seal-press'])).toBeLessThan(
      Number(MATERIALS.dark['seal-press'])
    );
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

/** A key's hex in a theme, by name (a key a test names before the palette declares it reads undefined). */
function ink(theme: (typeof THEMES)[number], key: string): string {
  return (PALETTE[theme] as Readonly<Record<string, string>>)[key];
}

// Readability plan 2026-11-18, ruling R5 (lane B2), re-pinned on purpose: one ink per amount. The
// digits after an amount's point were a step quieter than its integer, in the `-dim` ink of its
// sign (UI review 2026-11-14, R4, DY-06); a number's significant digits are never quieter than its
// integer now, so the table keeps one ink per sign, each at body contrast on every ground. The
// ghost stays for a column's padding zeros only (the ink gate holds it).
const AMOUNT_INKS = [
  { sign: 'positive', ink: 'jade', className: 'text-positive' },
  { sign: 'negative', ink: 'rouge', className: 'text-negative' },
  { sign: 'neutral', ink: 'text', className: 'text-text' },
] as const;

// Direction G (round 7, Strap.dc.html, StrapDay.dc.html): the proposal window is a gilt strap
// across the heads of its plates, so a block's number no longer turns gold inside it (the word
// gold would be 4.2 : 1 on the gilt by night, 3.8 by day): the number stays text-bright and the
// counts text. The word gold of lane E2 round 3 went with it.
describe('the pipeline gilt (direction G)', () => {
  it('takes the values the boards set, by night and by day, and the word gold is gone', () => {
    expect(PALETTE.dark['pipeline-gilt']).toBe('#564013');
    expect(PALETTE.light['pipeline-gilt']).toBe('#c8b47f');
    expect('pipeline-word-gold' in PALETTE.dark).toBe(false);
    expect('pipeline-word-gold' in PALETTE.light).toBe(false);
  });

  // The words a gilt plate's head line carries: its number (text-bright) and its "+N" (text).
  // text-dim is not used on the gilt (2.7 / 2.9 : 1), as it is not on the plate.
  it('reads the number and the counts on the gilt at body contrast, in both themes', () => {
    for (const theme of THEMES)
      for (const key of ['text-bright', 'text'] as const) {
        const ratio = contrastRatio(PALETTE[theme][key], PALETTE[theme]['pipeline-gilt']);
        expect(ratio, `${theme} ${key} on the gilt`).toBeGreaterThanOrEqual(4.5);
      }
    // measured for the ruling: text at 4.77 by day, and by night 6.24 (re-pinned on purpose: 4.58
    // before the night's reading ink was re-graded, readability ruling R2)
    expect(contrastRatio(PALETTE.dark.text, PALETTE.dark['pipeline-gilt'])).toBeCloseTo(6.24, 2);
    expect(contrastRatio(PALETTE.light.text, PALETTE.light['pipeline-gilt'])).toBeCloseTo(4.77, 2);
  });

  // Ruling R2: the same words measured perceptually (|Lc|), with the night's ruled inks.
  it('measures the number and the counts on the gilt perceptually, by night and by day', () => {
    const on = (theme: (typeof THEMES)[number], key: 'text-bright' | 'text') =>
      Math.abs(apcaLc(PALETTE[theme][key], PALETTE[theme]['pipeline-gilt']));
    // text-bright 81.54 (78.39 with the bright ink of 2026-11-10, before it moved for the
    // labelled bars' 4.5:1; re-pinned with the values)
    expect(on('dark', 'text-bright')).toBeCloseTo(81.54, 1);
    expect(on('dark', 'text')).toBeCloseTo(65.79, 1);
    expect(on('light', 'text-bright')).toBeCloseTo(62.2, 1);
    expect(on('light', 'text')).toBeCloseTo(50.78, 1);
  });

  it('stands the gilt off the plate it lies on by 1.25:1, in both themes', () => {
    for (const theme of THEMES)
      expect(
        contrastRatio(PALETTE[theme]['pipeline-gilt'], PALETTE[theme]['pipeline-plate']),
        theme
      ).toBeGreaterThanOrEqual(1.25);
  });
});

// Direction F (2026-11-17): a block is a solid plate of its own tone, every transaction set in a
// cut of a second tone (2 px round each square and the cellbase), the proposal dots on the plate.
// The values are the boards' (docs/superpowers/design/2026-11-17-pipeline-finish/project/Plate*);
// the day dots are re-derived against the day plate, where the old mixes fell to 2.9 and 2.6 : 1.
describe('the pipeline plate, its cut and its dots (direction F)', () => {
  /** `a` moved toward `b` by `t` in sRGB, channel by channel: what `color-mix(in srgb)` paints. */
  const mixHex = (a: string, b: string, t: number) => {
    const x = hexToRgb(a);
    const y = hexToRgb(b);
    const ch = (k: 'r' | 'g' | 'b') =>
      Math.round(x[k] * (1 - t) + y[k] * t)
        .toString(16)
        .padStart(2, '0');
    return `#${ch('r')}${ch('g')}${ch('b')}`;
  };

  it('takes the values the boards set, by night and by day', () => {
    const pairs: [string, string, string][] = [
      ['pipeline-plate', '#2f2821', '#ddd4c3'],
      ['pipeline-cut', '#13100d', '#f8f4ec'],
      ['pipeline-dot-waiting', '#ba9a5a', '#866619'],
      ['pipeline-dot-committed', '#63897f', '#3d7870'],
      ['pipeline-dot-expired', '#48413a', '#b9b3a8'],
    ];
    for (const [key, night, day] of pairs) {
      expect(ink('dark', key), `night ${key}`).toBe(night);
      expect(ink('light', key), `day ${key}`).toBe(day);
    }
  });

  it('reads the number and the counts on the plate at body contrast, in both themes', () => {
    for (const theme of THEMES)
      for (const key of ['text-bright', 'text'] as const) {
        const ratio = contrastRatio(PALETTE[theme][key], ink(theme, 'pipeline-plate'));
        expect(ratio, `${theme} ${key} on the plate`).toBeGreaterThanOrEqual(4.5);
      }
  });

  // Ruling R2: the same words measured perceptually (|Lc|), with the night's ruled inks.
  it('measures the number and the counts on the plate perceptually, by night and by day', () => {
    const on = (theme: (typeof THEMES)[number], key: 'text-bright' | 'text') =>
      Math.abs(apcaLc(PALETTE[theme][key], ink(theme, 'pipeline-plate')));
    // text-bright 88.91 (85.75 with the bright ink of 2026-11-10, before it moved for the
    // labelled bars' 4.5:1; re-pinned with the values)
    expect(on('dark', 'text-bright')).toBeCloseTo(88.91, 1);
    expect(on('dark', 'text')).toBeCloseTo(73.16, 1);
    expect(on('light', 'text-bright')).toBeCloseTo(79.41, 1);
    expect(on('light', 'text')).toBeCloseTo(67.99, 1);
  });

  // A step is the stage colour mixed toward the theme's step-to ink by STEP_W
  // (lib/pipeline/encoding.ts paintFromBase), the cellbase the committed colour a quarter of the
  // way there: lighter by night, inkier by day, so every step reads on the cut in both themes.
  it('sets every stage fill at every cycles step, and the cellbase, at 3:1 on the cut, in both themes', () => {
    const short: string[] = [];
    for (const theme of THEMES) {
      const cut = ink(theme, 'pipeline-cut');
      const to = PALETTE[theme]['pipeline-step-to'];
      for (const stage of ['pipeline-pending', 'pipeline-proposed', 'pipeline-committed'] as const)
        STEP_W.forEach((w, step) => {
          const fill = mixHex(PALETTE[theme][stage], to, w);
          const ratio = contrastRatio(fill, cut);
          if (ratio < 3) short.push(`${theme} ${stage} step ${step} ${fill}: ${ratio.toFixed(2)}`);
        });
      const cellbase = mixHex(PALETTE[theme]['pipeline-committed'], to, 0.25);
      const ratio = contrastRatio(cellbase, cut);
      if (ratio < 3) short.push(`${theme} cellbase ${cellbase}: ${ratio.toFixed(2)}`);
    }
    expect(short).toEqual([]);
    // the step-to is the light ink by night and the dark ink by day
    expect(PALETTE.dark['pipeline-step-to']).toBe('#f6f2ea');
    expect(PALETTE.light['pipeline-step-to']).toBe(PALETTE.light['text-bright']);
  });

  it('reads the waiting and the committed dot on the plate at 3:1; the expired one stays quiet', () => {
    for (const theme of THEMES) {
      const plate = ink(theme, 'pipeline-plate');
      for (const key of ['pipeline-dot-waiting', 'pipeline-dot-committed'])
        expect(contrastRatio(ink(theme, key), plate), `${theme} ${key}`).toBeGreaterThanOrEqual(3);
      expect(contrastRatio(ink(theme, 'pipeline-dot-expired'), plate), theme).toBeLessThan(
        contrastRatio(ink(theme, 'pipeline-dot-committed'), plate)
      );
    }
  });

  // Direction G: the window's gold is its one line of words now (text grade), and the muted ink
  // marks the pool's flows as arrowheads (a shape, 3:1).
  it('keeps the window’s gold words at 4.5:1 and the flows’ muted arrowheads at 3:1 on the page ground', () => {
    for (const theme of THEMES) {
      expect(
        contrastRatio(PALETTE[theme].gold, PALETTE[theme]['base-bg']),
        `${theme} gold`
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrastRatio(PALETTE[theme]['ink-muted'], PALETTE[theme]['base-bg']),
        `${theme} ink-muted`
      ).toBeGreaterThanOrEqual(3);
    }
  });

  it('stands the plate off the page ground by 1.25:1, in both themes', () => {
    for (const theme of THEMES)
      expect(
        contrastRatio(ink(theme, 'pipeline-plate'), PALETTE[theme]['base-bg']),
        theme
      ).toBeGreaterThanOrEqual(1.25);
  });
});

// Readability F1 follow-up: the "Who can change the code" bar on /scripts writes each class's name
// and share inside its fill, in the ink the fill-ink rule gives (inkOnFill). By night that ink reads
// at the night's word grade on every code-class fill; it did not on the unspendable grey, a mix of
// two inks that drifted to a mid grey (#8a8175) when the inks moved lighter: the rule's ink there
// read 5.06:1 but Lc 38, the bright ink 3.11:1 and Lc 56. By day the WCAG pin stands; the day
// pairs measure Lc 74.9 (base-void on the fixed grey #7a7163, 4.57:1), 60.4 (text-bright on the
// unspendable grey #bbb1a0, 8.07:1), 94.1 on the paper and 83.3 on the hair.
describe('words on the code-class fills (the scripts bar)', () => {
  const fills = [...new Set(Object.values(CODE_CLASS_KEYS).map((k) => k.fill))];

  it('reads the fill-ink rule’s ink on every code-class fill at Lc 60 or more by night', () => {
    const short: string[] = [];
    for (const fill of fills) {
      const { ink: key } = inkOnFill('dark', fill, 1);
      const lc = Math.abs(apcaLc(PALETTE.dark[key], PALETTE.dark[fill]));
      if (lc < 60) short.push(`${key} on ${fill}: Lc ${lc.toFixed(1)}`);
    }
    expect(short).toEqual([]);
  });

  it('reads it at 4.5:1 or more on every code-class fill in both themes', () => {
    for (const theme of THEMES)
      for (const fill of fills)
        expect(inkOnFill(theme, fill, 1).contrast, `${theme} ${fill}`).toBeGreaterThanOrEqual(4.5);
  });
});

describe('the amount inks (R5: one ink per amount)', () => {
  it('pins the table every amount renderer reads (AMOUNT_INK): one ink per sign, no fraction ink', () => {
    expect(AMOUNT_INK).toEqual(Object.fromEntries(AMOUNT_INKS.map((a) => [a.sign, a.className])));
  });

  it('reads every sign’s ink at body contrast on every ground, in both themes', () => {
    const short: string[] = [];
    for (const theme of THEMES)
      for (const { ink: key } of AMOUNT_INKS)
        for (const ground of GROUNDS) {
          const hex = ink(theme, key);
          expect(hex, `${theme} ${key}`).toMatch(HEX);
          const ratio = contrastRatio(hex, PALETTE[theme][ground]);
          if (ratio < 4.5) short.push(`${theme} ${key} on ${ground}: ${ratio.toFixed(2)}`);
        }
    expect(short).toEqual([]);
  });

  // Re-pinned on purpose (R5): this was "sets each fraction a step quieter than its sign's ink on
  // the page ground" (jade-dim under jade, rouge-dim under rouge, text-dim under text). The
  // decimals are never quieter than the integer now: no quieter step is an amount's ink.
  it('sets no amount’s decimals a step quieter: no `-dim` key is an amount ink', () => {
    for (const quieter of ['text-jade-dim', 'text-rouge-dim', 'text-text-dim'])
      expect(Object.values(AMOUNT_INK)).not.toContain(quieter);
  });

  it('keeps rouge-dim, moved in OKLCH lightness only until it reads at the text grade', () => {
    // rouge-dim was a secondary ink at 3:1 (#aa503c / #a6594c, 3.05 / 3.92 at worst) and used as
    // text; lightened by night (L +0.094) and darkened by day (L −0.033), hue and chroma kept, it
    // is the cinnabar's quiet step at text grade like every other `*-dim`: one key, one meaning.
    // It was the negative amount's fraction ink until R5; the key stays, a quiet cinnabar word's.
    // Re-pinned on purpose by night (readability ruling R2): the night's text grade is Lc 60 on
    // every ground, so the prototype's #f6947d (Lc 58) moved on in OKLCH lightness to #fc9982 (Lc
    // 60.1 on the surface, 60.9 on the page); it was #ca6c57 (4.51:1, Lc 38) before the ruling.
    expect(PALETTE.dark['rouge-dim']).toBe('#fc9982');
    expect(PALETTE.light['rouge-dim']).toBe('#9b4f43');
    expect(PALETTE_KEYS as string[]).not.toContain('rouge-fraction');
  });
});

// UI review 2026-11-14, ruling R5 (DY-01, DY-08): by day the page is ruled paper — the ink at the
// scanline alpha in 1 px of every 3, over the sky — and the hair (`base-border-subtle`, #e3dbcc)
// and the elevated tint (`base-elevated`, #e9e2d4) are 1.09:1 and 1.03:1 on a ruled line, so a row
// rule is one more ruled line and a bar's empty track is not there. A row rule and a bar track are
// keys of their own, each clearing the paper and a ruled line by 1.25:1; by night (readability
// ruling R2) the rule is the hair and the track a step of its own, at 1.25 and 1.30:1 to the
// ground, lighter than it as they were.
describe('the row rule and the bar track (R5, DY-01)', () => {
  const day = PALETTE.light;
  const ruledLine = overGround(
    'light',
    'text-bright',
    rgbaAlpha(MATERIALS.light['scanline-ink']),
    'base-bg'
  );

  it('computes the ruled line from the palette: the day ink at 4.5 % over the paper', () => {
    expect(MATERIALS.light['scanline-ink']).toBe(hexAlpha(day['text-bright'], 0.045));
    expect(ruledLine).toBe('#e9e5db');
    // the old keys are what the review measured: a rule and a track that are not there
    expect(contrastRatio(day['base-border-subtle'], ruledLine)).toBeLessThan(1.1);
    expect(contrastRatio(day['base-elevated'], ruledLine)).toBeLessThan(1.05);
  });

  it.each(['row-rule', 'bar-track'])(
    'clears the paper and a ruled line by 1.25:1 by day: %s',
    (key) => {
      const hex = ink('light', key);
      expect(hex, key).toMatch(HEX);
      expect(contrastRatio(hex, day['base-bg']), `${key} on the paper`).toBeGreaterThanOrEqual(
        1.25
      );
      expect(contrastRatio(hex, ruledLine), `${key} on a ruled line`).toBeGreaterThanOrEqual(1.25);
      // darker than both: a rule is ink on paper, never a lighter line
      expect(relativeLuminance(hex)).toBeLessThan(relativeLuminance(ruledLine));
    }
  );

  it('derives them from the hair and the elevated tint, darker in OKLCH lightness only', () => {
    expect(ink('light', 'row-rule')).toBe('#d5cdbf');
    expect(ink('light', 'bar-track')).toBe('#d4cdc0');
  });

  // Re-pinned on purpose (readability ruling R2, the owner's Gate F0): by night the row rule is
  // still the hair, both now at 1.25:1, and the bar's track is no longer the elevated tint (1.14:1,
  // and 1.17:1 since the tint moved) but a key of its own at 1.30:1; both stay lighter than the
  // ground. Before, this read "keeps them the boards' hair and elevated tint by night", ≥ 1.1:1.
  it('keeps the row rule the hair by night and the track its own step, lighter than the ground by 1.25:1', () => {
    expect(ink('dark', 'row-rule')).toBe(PALETTE.dark['base-border-subtle']);
    expect(ink('dark', 'bar-track')).toBe('#2f2924');
    for (const key of ['row-rule', 'bar-track']) {
      const hex = ink('dark', key);
      expect(relativeLuminance(hex), key).toBeGreaterThan(
        relativeLuminance(PALETTE.dark['base-bg'])
      );
      expect(contrastRatio(hex, PALETTE.dark['base-bg']), key).toBeGreaterThanOrEqual(1.25);
    }
  });
});

/** A material `rgba(r, g, b, a)` laid over a ground key, as the eye sees it. */
function materialOver(theme: (typeof THEMES)[number], value: string, ground: PaletteKey) {
  const m = value.match(/^rgba\((\d+), (\d+), (\d+), ([\d.]+)\)$/);
  if (!m) throw new Error(`${value} is not rgba(r, g, b, a)`);
  const a = Number(m[4]);
  const under = hexToRgb(PALETTE[theme][ground]);
  const ch = (f: string, g: number) =>
    Math.round(a * Number(f) + (1 - a) * g)
      .toString(16)
      .padStart(2, '0');
  return `#${ch(m[1], under.r)}${ch(m[2], under.g)}${ch(m[3], under.b)}`;
}

// Ruling 4b (UI review 2026-11-14): the ground decides its rule. The evidence tier lies on its own
// ground (`tier2-ground`, the elevated tint at 90 % by day over the sky), where the day row rule and
// track read 1.24:1 over the ground and 1.14:1 over a ruled line on it. Inside `.tier2-ground` the
// variables behind `row-rule` and `bar-track` resolve to deeper keys that clear both by 1.25:1;
// the whole site's rule is not darkened (it would pass the frame border, `base-border`).
describe('the row rule and the bar track on the tier-2 ground (R5, ruling 4b)', () => {
  const dayGround = materialOver('light', MATERIALS.light['tier2-ground'], 'base-bg');
  const scan = MATERIALS.light['scanline-ink'].match(/^rgba\((\d+), (\d+), (\d+), ([\d.]+)\)$/)!;
  const dayRuled = (() => {
    const a = Number(scan[4]);
    const g = hexToRgb(dayGround);
    const ch = (f: string, under: number) =>
      Math.round(a * Number(f) + (1 - a) * under)
        .toString(16)
        .padStart(2, '0');
    return `#${ch(scan[1], g.r)}${ch(scan[2], g.g)}${ch(scan[3], g.b)}`;
  })();

  it('computes the day tier-2 ground and its ruled line from the palette', () => {
    expect(dayGround).toBe('#eae3d6');
    expect(dayRuled).toBe('#e1dacd');
    // the tier-1 keys fall short there
    expect(contrastRatio(PALETTE.light['row-rule'], dayRuled)).toBeLessThan(1.25);
    expect(contrastRatio(PALETTE.light['bar-track'], dayRuled)).toBeLessThan(1.25);
  });

  it.each(['row-rule-deep', 'bar-track-deep'])(
    'clears the tier-2 ground and a ruled line on it by 1.25:1 by day: %s',
    (key) => {
      const hex = ink('light', key);
      expect(hex, key).toMatch(HEX);
      expect(contrastRatio(hex, dayGround), `${key} on the ground`).toBeGreaterThanOrEqual(1.25);
      expect(contrastRatio(hex, dayRuled), `${key} on a ruled line`).toBeGreaterThanOrEqual(1.25);
    }
  );

  it('derives them from the tier-1 keys, darker in OKLCH lightness only (L −0.030)', () => {
    expect(ink('light', 'row-rule-deep')).toBe('#cbc3b5');
    expect(ink('light', 'bar-track-deep')).toBe('#cac3b6');
  });

  it('keeps the tier-1 values by night, where they clear the tier-2 ground by 1.1:1 or more', () => {
    const nightGround = materialOver('dark', MATERIALS.dark['tier2-ground'], 'base-bg');
    expect(nightGround).toBe('#100e0b');
    for (const [deep, flat] of [
      ['row-rule-deep', 'row-rule'],
      ['bar-track-deep', 'bar-track'],
    ] as const) {
      expect(ink('dark', deep), deep).toBe(PALETTE.dark[flat]);
      expect(contrastRatio(ink('dark', deep), nightGround), deep).toBeGreaterThanOrEqual(1.1);
    }
  });
});

// Lane E2 round 3 (ruling): the ELF region hues are text. They paint the hex digits of a code
// cell's bytes in the cell microscope, which sits in the evidence tier, so besides the four grounds
// (TEXT_GRADE) each reads at 4.5:1 on the tier-2 ground (`tier2-ground` over the sky), in both
// themes. A key short of it moves in OKLCH lightness only, hue and chroma kept.
describe('the ELF region hues on the evidence tier', () => {
  const ELF_KEYS = [
    'elf-header',
    'elf-program-headers',
    'elf-rodata',
    'elf-eh-frame',
    'elf-data',
    'elf-meta',
  ] as const;

  it('reads at body-text contrast on the tier-2 ground, in both themes', () => {
    const short: string[] = [];
    for (const theme of THEMES) {
      const ground = materialOver(theme, MATERIALS[theme]['tier2-ground'], 'base-bg');
      for (const key of ELF_KEYS) {
        const ratio = contrastRatio(PALETTE[theme][key], ground);
        if (ratio < 4.5) short.push(`${theme} ${key} on tier-2 ${ground}: ${ratio.toFixed(2)}`);
      }
    }
    expect(short).toEqual([]);
  });

  it('moves only the day keys that fell short, darker in OKLCH lightness only (hue and chroma kept)', () => {
    // rodata L −0.007, eh-frame L −0.030, data L −0.021, meta L −0.041 (from #3d7249, #5f7362,
    // #82672c, #7468a7); the header cleared and stays; every night value cleared and stays
    expect(PALETTE.light['elf-rodata']).toBe('#3b7047');
    expect(PALETTE.light['elf-eh-frame']).toBe('#576a5a');
    expect(PALETTE.light['elf-data']).toBe('#7c6126');
    expect(PALETTE.light['elf-meta']).toBe('#695c9a');
    expect(PALETTE.light['elf-header']).toBe('#416a7a');
    // round 4: the header's hue and chroma at L 0.430 (the night pair's 0.0706 below the header)
    expect(PALETTE.light['elf-program-headers']).toBe('#2d5665');
    // Re-pinned on purpose by night (readability ruling R2): the header (the data framing) and the
    // meta moved in OKLCH lightness to Lc 60 on every word ground, and the program headers are the
    // header's hue a step lighter again (L +0.0704, inside the day pair's 0.0708); rodata, eh-frame
    // and data already cleared Lc 60 and stay. Before: #7898a3, #8daeb9, …, #b8acd0.
    expect([
      PALETTE.dark['elf-header'],
      PALETTE.dark['elf-program-headers'],
      PALETTE.dark['elf-rodata'],
      PALETTE.dark['elf-eh-frame'],
      PALETTE.dark['elf-data'],
      PALETTE.dark['elf-meta'],
    ]).toEqual(['#97b9c4', '#abd0dc', '#9ac09b', '#aab9aa', '#d5b985', '#baaed2']);
  });

  // Round 4's ruling: by night the program headers are the header's hue a step lighter; by day a
  // lighter step cannot be text grade (at 4.5:1 it lands on the header), so, as the `*-bright`
  // keys do, the step turns: by day the header's hue and chroma a step darker, spaced from the
  // header as the night pair is.
  it('makes the day program headers the header a step darker, as far as the night pair is apart', () => {
    const night =
      oklch(PALETTE.dark['elf-program-headers']).L - oklch(PALETTE.dark['elf-header']).L;
    expect(night).toBeGreaterThan(0.07);
    const header = oklch(PALETTE.light['elf-header']);
    const program = oklch(PALETTE.light['elf-program-headers']);
    expect(header.L - program.L).toBeGreaterThanOrEqual(night);
    // the header's hue and chroma, to the rounding of a hex at this low chroma
    expect(Math.abs(program.C - header.C)).toBeLessThan(0.002);
    expect(Math.abs(program.hue - header.hue)).toBeLessThan(1.5);
  });
});

/** A hex in OKLCH (Björn Ottosson's OKLab): lightness, chroma and hue in degrees. */
function oklch(hex: string): { L: number; C: number; hue: number } {
  const lin = (v: number) => {
    const c = v / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const { r, g, b } = hexToRgb(hex);
  const [R, G, B] = [lin(r), lin(g), lin(b)];
  const l = Math.cbrt(0.4122214708 * R + 0.5363325363 * G + 0.0514459929 * B);
  const m = Math.cbrt(0.2119034982 * R + 0.6806995451 * G + 0.1073969566 * B);
  const s = Math.cbrt(0.0883024619 * R + 0.2817188376 * G + 0.6299787005 * B);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const Bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return { L, C: Math.hypot(A, Bb), hue: (Math.atan2(Bb, A) * 180) / Math.PI };
}

describe('the palette as CSS', () => {
  it('declares a colour, its channels and the materials per theme', () => {
    const dark = themeDeclarations('dark');
    expect(dark['--c-jade']).toBe(RUBBING.jade);
    expect(dark['--rgb-jade']).toBe('158 205 194');
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
    expect(hexAlpha(RUBBING.jade, 0.2)).toBe('rgba(158, 205, 194, 0.2)');
    expect(() => hexAlpha(color('jade'), 0.2)).toThrow('not #rrggbb');
  });

  it('parses hex and refuses what is not #rrggbb', () => {
    expect(hexToRgb(RUBBING.jade)).toEqual({ r: 158, g: 205, b: 194 });
    expect(() => hexToRgb('#fff')).toThrow('#fff is not #rrggbb');
    expect(() => hexToRgb('var(--c-jade)')).toThrow('not #rrggbb');
    // white on black is 21:1, the scale's end
    expect(contrastRatio('#ffffff', '#000000')).toBeCloseTo(21, 5);
  });
});

// Readability plan 2026-11-18, ruling R2: the night text grade is perceptual, APCA Lc on the ground
// an ink is used on. apcaLc is APCA-W3 0.0.98G-4g, the implementation the review measured with
// (docs/superpowers/reviews/2026-11-18-readability-review/analyze_read.py, apca_y and apca).
describe('apcaLc: the perceptual contrast (APCA-W3 0.0.98G-4g)', () => {
  // the published reference pairs, text on background → Lc
  it.each([
    ['#888888', '#ffffff', 63.06],
    ['#ffffff', '#888888', -68.54],
    ['#000000', '#aaaaaa', 58.15],
    ['#aaaaaa', '#000000', -56.24],
    ['#112233', '#ddeeff', 91.67],
    ['#ddeeff', '#112233', -93.07],
  ])('reads %s on %s at Lc %s', (text, ground, lc) => {
    expect(apcaLc(text, ground)).toBeCloseTo(lc, 2);
  });

  it('is signed by polarity, its grade the absolute value, and zero for an ink on itself', () => {
    // dark ink on a light ground is positive, light ink on a dark ground negative
    expect(apcaLc('#000000', '#ffffff')).toBeGreaterThan(100);
    expect(apcaLc('#ffffff', '#000000')).toBeLessThan(-100);
    expect(apcaLc('#15120f', '#15120f')).toBe(0);
    // under the low clamp (|Lc| below 10 before the offset) there is no contrast to speak of
    expect(apcaLc('#16130f', '#15120f')).toBe(0);
    expect(() => apcaLc('#fff', '#000000')).toThrow('#fff is not #rrggbb');
  });
});

/** The perceptual grade (|Lc|) of a key read on a ground key, in a theme. */
function lcOn(theme: (typeof THEMES)[number], key: PaletteKey, ground: PaletteKey): number {
  return Math.abs(apcaLc(PALETTE[theme][key], PALETTE[theme][ground]));
}

/** The grounds the full floors hold on (the page, the surface, the well); the elevated tint is a little lower. */
const FLOOR_GROUNDS: PaletteKey[] = ['base-bg', 'base-surface', 'base-void'];

/** Every ink that carries a word besides the bright and the reading ink: the quiet ink, every accent, class, cell part and region of the text grade. */
const WORD_INKS = TEXT_GRADE.filter((key) => key !== 'text-bright' && key !== 'text');

/** The inks of marks, never words, from the quietest up. */
const MARK_INKS = ['text-ghost', 'ink-quiet', 'ink-muted'] as const satisfies readonly PaletteKey[];

// Readability plan 2026-11-18, ruling R2 (the owner's Gate F0: the night ladder as the prototype
// showed it, accents at Lc 70): a word is graded by its perceptual contrast on the ground it is
// read on. The reading ink reads at Lc 75 or more and every other word ink at 60 or more on the
// page, the surface and the well, at 72 and 58 on the elevated tint; the marks stay under 60. The
// floors are the night's grade, and a floor the day values already clear: the day keeps its
// values. WCAG's 4.5:1 (above) stays the second floor.
describe('the text grade by perceptual contrast (ruling R2)', () => {
  it.each(THEMES)(
    'reads the reading ink at Lc 75 and every other word ink at Lc 60 on the page, the surface and the well, 72 and 58 on the elevated tint: %s',
    (theme) => {
      const short: string[] = [];
      const floor = (key: PaletteKey, ground: PaletteKey, min: number) => {
        const lc = lcOn(theme, key, ground);
        if (lc < min) short.push(`${key} on ${ground}: Lc ${lc.toFixed(1)} < ${min}`);
      };
      for (const ground of FLOOR_GROUNDS) {
        floor('text', ground, 75);
        for (const key of WORD_INKS) floor(key, ground, 60);
      }
      floor('text', 'base-elevated', 72);
      for (const key of WORD_INKS) floor(key, 'base-elevated', 58);
      expect(short).toEqual([]);
    }
  );

  it.each(THEMES)(
    'sets the bright, the reading and the quiet ink at least 10 Lc apart, in that order, on every ground: %s',
    (theme) => {
      for (const ground of GROUNDS) {
        const bright = lcOn(theme, 'text-bright', ground);
        const reading = lcOn(theme, 'text', ground);
        const quiet = lcOn(theme, 'text-dim', ground);
        expect(bright - reading, `bright over reading on ${ground}`).toBeGreaterThanOrEqual(10);
        expect(reading - quiet, `reading over quiet on ${ground}`).toBeGreaterThanOrEqual(10);
      }
    }
  );

  // By day the muted ink reads over the word floor on the paper, the sheet and the well (Lc 64.4,
  // 69.6, 70.7; 57.5 on the elevated tint), as it did before this ruling: the day values are not
  // this ruling's to move, so it is reported (readability lane F1) and held here as the one known
  // exception, which a fix of the day ink or a new exception will both show.
  it('keeps the ghost, the quiet and the muted ink under Lc 60 on every ground in both themes, in rising order: marks, never words', () => {
    const over: string[] = [];
    for (const theme of THEMES)
      for (const ground of GROUNDS) {
        const marks = MARK_INKS.map((key) => lcOn(theme, key, ground));
        marks.forEach((lc, i) => {
          if (lc >= 60) over.push(`${theme} ${MARK_INKS[i]} on ${ground}`);
        });
        expect(marks[0], `${theme} ghost under quiet on ${ground}`).toBeLessThan(marks[1]);
        expect(marks[1], `${theme} quiet under muted on ${ground}`).toBeLessThan(marks[2]);
      }
    expect(over).toEqual([
      'light ink-muted on base-void',
      'light ink-muted on base-bg',
      'light ink-muted on base-surface',
    ]);
  });

  // An accent's quiet step (`-dim`) reads under it and its bright step (`-bright`) over it. By day
  // the old gold's quiet step reads a hair over the gold itself (Lc 69.3 against 69.1 on the paper,
  // on every ground), as it did before this ruling: reported (readability lane F1) and held here as
  // the one known exception.
  it('reads an accent’s -dim step under it and its -bright step over it, on every ground in both themes', () => {
    const steps = (suffix: '-dim' | '-bright') =>
      PALETTE_KEYS.filter((key) => key.endsWith(suffix) && key !== `text${suffix}`).map((key) => {
        const accent = key.slice(0, -suffix.length) as PaletteKey;
        expect(PALETTE_KEYS, `${key}'s accent`).toContain(accent);
        return [key, accent] as const;
      });
    const dims = steps('-dim');
    const brights = steps('-bright');
    expect(dims.map(([key]) => key).sort()).toEqual([
      'amber-dim',
      'aqua-dim',
      'gold-dim',
      'jade-dim',
      'lavender-dim',
      'rouge-dim',
    ]);
    expect(brights.map(([key]) => key).sort()).toEqual([
      'aqua-bright',
      'gold-bright',
      'jade-bright',
      'rouge-bright',
      'token-bright',
    ]);
    const wrong: string[] = [];
    for (const theme of THEMES)
      for (const ground of GROUNDS) {
        for (const [dim, accent] of dims)
          if (!(lcOn(theme, dim, ground) < lcOn(theme, accent, ground)))
            wrong.push(`${theme} ${dim} on ${ground}`);
        for (const [bright, accent] of brights)
          if (!(lcOn(theme, bright, ground) > lcOn(theme, accent, ground)))
            wrong.push(`${theme} ${bright} on ${ground}`);
      }
    expect(wrong).toEqual([
      'light gold-dim on base-void',
      'light gold-dim on base-bg',
      'light gold-dim on base-surface',
      'light gold-dim on base-elevated',
    ]);
  });

  // By night a rule, a track and a frame take the day's rule (R5, below): a row rule, the hair and a
  // bar's track clear the page ground by 1.25:1, a frame by 1.40:1 (the owner's Gate F0); before,
  // they were the boards' hair (1.11:1), elevated tint (1.14:1) and line (1.21:1).
  it.each([
    ['row-rule', 1.25],
    ['row-rule-deep', 1.25],
    ['base-border-subtle', 1.25],
    ['bar-track', 1.25],
    ['bar-track-deep', 1.25],
    ['base-border', 1.4],
  ] as const)('stands %s off the night page ground by %s:1', (key, ratio) => {
    expect(contrastRatio(PALETTE.dark[key], PALETTE.dark['base-bg'])).toBeGreaterThanOrEqual(ratio);
  });
});
