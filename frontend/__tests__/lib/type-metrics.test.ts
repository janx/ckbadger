import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import tailwindConfig from '@/tailwind.config';
import { TYPE_SCALE, TYPE_STEPS } from '@/lib/type-scale';
import { cn } from '@/lib/utils';
import { cmapCodePoints, woff2Tables } from '@/__tests__/utils/woff2';
import {
  LABEL_LINE_PX,
  MEASURE_CHARS,
  MONO_ADVANCE_EM,
  MONO_DESCENT_EM,
  PHONE_LINE_PX,
  PHONE_PART_GAP_PX,
  ROW_LINE_PX,
  advancePx,
  charPx,
  columnPx,
  descentPx,
  emUnitsPx,
  fontPx,
  foreignGraphemes,
  foreignTextPx,
  lineHeightPx,
  phoneHead,
  textPx,
  tightLinePx,
} from '@/lib/type-metrics';

/**
 * The metrics every text budget is computed from (readability plan 2026-11-18, lane F3, step 1):
 * the face's advance read from the font files the site serves, and each size and line read from
 * the type scale, so a change of the scale moves every budget with it.
 */

const FONTS = join(__dirname, '..', '..', 'public', 'fonts');

describe('the face (JetBrains Mono, public/fonts)', () => {
  const faces = readdirSync(FONTS).filter((name) => /^jetbrains-mono-.*\.woff2$/.test(name));

  it('serves the five JetBrains Mono faces the stylesheet declares', () => {
    expect(faces.sort()).toEqual([
      'jetbrains-mono-italic.woff2',
      'jetbrains-mono-light.woff2',
      'jetbrains-mono-medium.woff2',
      'jetbrains-mono-regular.woff2',
      'jetbrains-mono-semibold.woff2',
    ]);
  });

  it.each(faces)('%s advances every glyph 0.6 em: MONO_ADVANCE_EM is the font’s', (face) => {
    const t = woff2Tables(join(FONTS, face));
    const unitsPerEm = t.get('head')!.readUInt16BE(18);
    const advanceWidthMax = t.get('hhea')!.readUInt16BE(10);
    const xAvgCharWidth = t.get('OS/2')!.readInt16BE(2);
    const isFixedPitch = t.get('post')!.readUInt32BE(12);
    expect(isFixedPitch, 'post.isFixedPitch').not.toBe(0);
    expect(xAvgCharWidth, 'OS/2.xAvgCharWidth').toBe(advanceWidthMax);
    expect(advanceWidthMax / unitsPerEm).toBe(MONO_ADVANCE_EM);
    // and reaches 0.3 em under its baseline: hhea.descender
    expect(-t.get('hhea')!.readInt16BE(6) / unitsPerEm).toBe(MONO_DESCENT_EM);
  });

  it('gives a line’s descent at a size: 0.3 em', () => {
    expect([10, 12, 14].map(descentPx)).toEqual([3, 3.6, 4.2]);
  });
});

// Review F-B (logic finding 5): text that is not ours is measured by an upper bound, a grapheme
// of the served faces' shared (Latin) subset at 0.6 em and any other at 1.25 em; the bound is
// checked here against the five files and against what each kind of string is.
describe('text that is not ours: an upper bound by grapheme', () => {
  const faces = readdirSync(FONTS).filter((name) => /^jetbrains-mono-.*\.woff2$/.test(name));

  it('counts at 0.6 em exactly the code points all five served faces map', () => {
    const maps = faces.map((face) => cmapCodePoints(woff2Tables(join(FONTS, face))));
    const shared = [...maps[0]].filter((cp) => maps.every((m) => m.has(cp))).sort((a, b) => a - b);
    // the semibold, light and italic subsets: 229 glyphs, all in the regular and medium faces
    expect(shared).toHaveLength(229);
    expect(Math.max(...shared)).toBeLessThan(0x10000);
    // every other code point of the basic plane (a lone surrogate is not a character) at 1.25 em
    const counted: number[] = [];
    const neither: string[] = [];
    for (let cp = 0; cp < 0x10000; cp++) {
      if (cp >= 0xd800 && cp <= 0xdfff) continue;
      const px = foreignTextPx(String.fromCodePoint(cp), 10);
      if (px === 6) counted.push(cp);
      else if (px !== 12.5) neither.push(`${cp.toString(16)}: ${px}`);
    }
    expect(counted).toEqual(shared);
    expect(neither).toEqual([]);
  });

  it('measures a Latin text exactly as textPx does', () => {
    for (const [text, px] of [
      ['Nervape', 13],
      ['bytes 51–52 · nervapeColor', 12.5],
      ['Hetzner 50.00%', 13],
      ['café', 12],
    ] as const)
      expect(foreignTextPx(text, px), text).toBe(textPx(Array.from(text).length, px));
    expect(foreignTextPx('', 12)).toBe(0);
  });

  it('counts a CJK character, an emoji however composed and a symbol the subsets lack at 1.25 em', () => {
    // two CJK characters at 13 px: 2 × 16.25
    expect(foreignTextPx('颜色', 13)).toBe(32.5);
    // a thumb with a skin-tone modifier, a family joined by ZWJ, a flag: one grapheme each, 15 px
    // at 12 px
    for (const emoji of ['👍🏽', '👨‍👩‍👧', '🇨🇳', '🐱'])
      expect(foreignGraphemes(emoji), emoji).toEqual([{ grapheme: emoji, units: 1250 }]);
    expect(foreignTextPx('👨‍👩‍👧', 12)).toBe(15);
    // → is in the regular and medium faces only: a semibold label would fall back
    expect(foreignTextPx('→', 12)).toBe(15);
    // a full-width letter
    expect(foreignTextPx('Ａ', 12)).toBe(15);
  });

  it('measures a mixed string grapheme by grapheme', () => {
    // "Nervape " (8), 猫, " " (1), 🐱: 9 Latin glyphs and 2 others at 12 px: 64.8 + 30
    expect(foreignTextPx('Nervape 猫 🐱', 12)).toBe(94.8);
    expect(foreignGraphemes('a猫🐱').map((g) => g.units)).toEqual([600, 1250, 1250]);
  });

  it('counts a letter and its combining accent as one grapheme of the subset', () => {
    // e and U+0301, both in the subset: one grapheme, 0.6 em; the precomposed é is too
    expect(foreignGraphemes('é')).toEqual([{ grapheme: 'é', units: 600 }]);
    expect(foreignTextPx('é', 12)).toBe(7.2);
    expect(foreignTextPx('é', 12)).toBe(7.2);
    // an accent the subset does not hold (U+0302) takes its letter to the fallback bound
    expect(foreignTextPx('ê', 12)).toBe(15);
  });
});

// Review fb-logic N3 (its second part): one path from thousandths of an em to px, which
// foreignTextPx and clipForeignLabel (lib/shorten.ts) both take.
describe('thousandths of an em', () => {
  it('turns them into px at a size by one function', () => {
    expect(emUnitsPx(600, 12)).toBe(7.2);
    expect(emUnitsPx(1250, 12)).toBe(15);
    expect(emUnitsPx(600 + 1250, 13)).toBe(foreignTextPx('a猫', 13));
  });
});

// Review fb-logic N7 (lane rd/fb-fixes-2): the segmenter was built when the module loaded, and
// every page loads this module (and tailwind.config.ts does), so a browser without
// Intl.Segmenter (Firefox before 125, Safari before 14.1) got no page at all. It is built where
// text that is not ours is first measured, once; where it is missing that figure throws and says
// so, and nothing else is touched. No other segmentation stands in for it.
describe('the grapheme segmenter', () => {
  it('is built where text that is not ours is first measured, not when the module loads', async () => {
    const Real = Intl.Segmenter;
    let built = 0;
    class Counted extends Real {
      constructor(...args: ConstructorParameters<typeof Intl.Segmenter>) {
        super(...args);
        built++;
      }
    }
    try {
      vi.resetModules();
      vi.stubGlobal('Intl', Object.create(Intl, { Segmenter: { value: Counted } }));
      const fresh = await import('@/lib/type-metrics');
      expect(built).toBe(0);
      expect(fresh.foreignTextPx('猫', 12)).toBe(15);
      expect(fresh.foreignGraphemes('a🐱')).toHaveLength(2);
      expect(built).toBe(1);

      vi.resetModules();
      vi.stubGlobal('Intl', Object.create(Intl, { Segmenter: { value: undefined } }));
      const bare = await import('@/lib/type-metrics');
      // every figure of our own text is measured as before
      expect(bare.textPx(13, 12)).toBe(93.6);
      expect(() => bare.foreignTextPx('猫', 12)).toThrow(/Intl\.Segmenter/);
    } finally {
      vi.unstubAllGlobals();
      vi.resetModules();
    }
  });
});

describe('sizes and advances', () => {
  it('reads every step’s size from the type scale', () => {
    for (const step of TYPE_STEPS) expect(fontPx(step), step).toBe(parseFloat(TYPE_SCALE[step][0]));
    expect([fontPx('meta'), fontPx('small'), fontPx('body')]).toEqual([12, 13, 14]);
  });

  it('gives a glyph’s advance at a size as the nearest double of 0.6 em', () => {
    // 0.6 × 12 is 7.199999999999999 in binary floating point; 600 × 12 / 1000 is 7.2
    expect([10, 10.5, 11, 11.5, 12, 12.5, 13].map(advancePx)).toEqual([
      6, 6.3, 6.6, 6.9, 7.2, 7.5, 7.8,
    ]);
  });

  it('gives a step’s advance as the advance at its size', () => {
    for (const step of TYPE_STEPS) expect(charPx(step), step).toBe(advancePx(fontPx(step)));
    expect([charPx('meta'), charPx('small'), charPx('body')]).toEqual([7.2, 7.8, 8.4]);
  });

  it('gives a run of glyphs as one product, exact for a whole count', () => {
    expect(textPx(13, 12)).toBe(93.6); // "3,500,000,000" at text-small (13 × 7.2 adds to 93.60000000000001)
    expect(textPx(11, 13)).toBe(85.8);
    expect(textPx(34, 13)).toBe(265.2);
    expect(textPx(0, 11)).toBe(0);
    for (let n = 1; n <= 80; n++) expect(textPx(n, 12)).toBeCloseTo(n * charPx('meta'), 10);
  });
});

describe('lines', () => {
  it('reads every step’s line from the type scale', () => {
    for (const step of TYPE_STEPS)
      expect(lineHeightPx(step), step).toBeCloseTo(
        fontPx(step) * parseFloat(TYPE_SCALE[step][1]),
        10
      );
    expect([lineHeightPx('meta'), lineHeightPx('small'), lineHeightPx('body')]).toEqual([
      18, 19.5, 22.4,
    ]);
  });

  it('stands a list row on the small step’s line, in whole pixels: 20 px', () => {
    expect(ROW_LINE_PX).toBe(Math.ceil(lineHeightPx('small')));
    expect(Number.isInteger(ROW_LINE_PX)).toBe(true);
    expect(ROW_LINE_PX).toBe(20); // 13 × 1.5 = 19.5, up to a whole pixel
  });

  it('sets a line tight with 3 px of leading, and a label at the meta step so: 15 px', () => {
    expect([11, 12, 13].map(tightLinePx)).toEqual([14, 15, 16]);
    expect(LABEL_LINE_PX).toBe(tightLinePx(fontPx('meta')));
    expect(LABEL_LINE_PX).toBe(15);
  });

  it('emits both lines as Tailwind line heights: leading-row and leading-label', () => {
    const theme = tailwindConfig.theme as { extend: { lineHeight?: Record<string, string> } };
    expect(theme.extend.lineHeight).toEqual({
      row: `${ROW_LINE_PX}px`,
      label: `${LABEL_LINE_PX}px`,
    });
  });

  // readability R6: a row stone stands centred in a box one row line tall (RowSeal's `h-row`)
  it('emits the row line as a Tailwind height too, h-row, which cn() merges as a height', () => {
    const theme = tailwindConfig.theme as { extend: { height?: Record<string, string> } };
    expect(theme.extend.height).toEqual({ row: `${ROW_LINE_PX}px` });
    expect(cn('h-5', 'h-row')).toBe('h-row');
    expect(cn('h-row', 'leading-row')).toBe('h-row leading-row');
  });
});

describe('the measure (readability R4, lane B7)', () => {
  it('holds 76 characters of running text, 638.4 px at the body step', () => {
    expect(MEASURE_CHARS).toBe(76);
    // the captions were 640 px: at the body step the measure keeps their width within a glyph
    expect(textPx(MEASURE_CHARS, fontPx('body'))).toBe(638.4);
    expect(textPx(MEASURE_CHARS, fontPx('small'))).toBe(592.8);
    expect(textPx(MEASURE_CHARS, fontPx('meta'))).toBe(547.2);
  });

  it('is written in characters, `max-w-measure`: 76ch, the advance of the face at any step', () => {
    // `ch` is the advance of the "0" of the element's face; every glyph of JetBrains Mono is that
    // wide (0.6 em, the face's tests above), so 76ch is textPx(76, size) at every step
    expect(MONO_ADVANCE_EM).toBe(0.6);
    const theme = tailwindConfig.theme as { extend: { maxWidth?: Record<string, string> } };
    expect(theme.extend.maxWidth).toEqual({ measure: '76ch' });
  });
});

describe('column budgets', () => {
  it('rounds what a column holds up to a whole pixel and adds its spare', () => {
    expect(columnPx(textPx(13, 12))).toBe(94); // 93.6
    expect(columnPx(textPx(20, 12) + 16)).toBe(160); // exactly 160 stays 160
    expect(columnPx(56.5 + textPx(17, 13) + 16, 2)).toBe(208); // 205.1 → 206, and 2 spare
    expect(columnPx(160.00000000000003)).toBe(160); // a sum a hair over a whole pixel is that pixel
  });
});

// Lane D4 (the phone-rows board the owner passed at gate D0′): a list states its row's first line
// against a 360 px phone's line, as the board's head() did, and does not load with a wider one.
describe('a phone row’s first line', () => {
  it('stands on a phone’s content line: 328 px at 360, 358 at 390', () => {
    // the window less its two 16 px gutters (the container's px-4 below sm)
    expect(PHONE_LINE_PX).toEqual({ 360: 360 - 32, 390: 390 - 32 });
    expect(PHONE_PART_GAP_PX).toBe(12);
  });

  it('states a line that fits, to the hundredth, and refuses one a hair wider than 328 px', () => {
    // the board's blocks head: "#21,009,134" at 14 px beside "1 tx · 20s ago" at 13 px, a 16 px
    // stone and its gap (238 of 328 there with its 10 px gap; 240 with PhoneRow's 12)
    expect(phoneHead('blocks', 24 + textPx(11, 14) + 12 + textPx(14, 13))).toBe(237.6);
    expect(phoneHead('exactly', 328)).toBe(328);
    expect(() => phoneHead('one glyph over', 328 - textPx(1, 13) + textPx(2, 13))).toThrow(
      'phone row "one glyph over": its first line is 335.8 px, wider than a 360 px phone\'s 328 px line'
    );
    expect(() => phoneHead('not a width', Number.NaN)).toThrow('not a width');
  });
});
