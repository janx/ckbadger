import { describe, expect, it } from 'vitest';
import {
  SVG_TEXT,
  SVG_TEXT_FLOOR,
  centreShift,
  lineAscentPx,
  wrapPhrases,
  wrapWords,
} from '@/lib/chart-type';
import { descentPx, fontPx, tightLinePx } from '@/lib/type-metrics';

/**
 * SVG text by role (readability plan 2026-11-18, lane F3, step 1): the sizes the figures used as
 * literals, now read from the type scale, so the table moves with it.
 */
describe('SVG text by role', () => {
  // readability R4 (step 2 of lane F3): tick 10, note 10.5 and label 11 are the meta step, 12;
  // value 11.5, head 12, title 12.5 and reading 13 moved with their steps to 12.5, 13, 13.5, 14
  it('gives the sizes the figures are drawn at, smallest first', () => {
    expect(SVG_TEXT).toEqual({
      tick: 12,
      note: 12,
      label: 12,
      value: 12.5,
      head: 13,
      // the ink and size roles, finished: a sentence in a plot is prose, 13 px
      sentence: 13,
      title: 13.5,
      reading: 14,
    });
  });

  it('sets no role under the 12 px floor, the meta step (R4)', () => {
    expect(SVG_TEXT_FLOOR).toBe(fontPx('meta'));
    expect(SVG_TEXT_FLOOR).toBe(12);
    for (const px of Object.values(SVG_TEXT)) expect(px).toBeGreaterThanOrEqual(SVG_TEXT_FLOOR);
  });

  it('writes every role as a step of the scale and the pixels it stands below it', () => {
    expect(SVG_TEXT.tick).toBe(fontPx('meta'));
    expect(SVG_TEXT.note).toBe(fontPx('meta'));
    expect(SVG_TEXT.label).toBe(fontPx('meta'));
    expect(SVG_TEXT.value).toBe(fontPx('small') - 0.5);
    expect(SVG_TEXT.head).toBe(fontPx('small'));
    expect(SVG_TEXT.title).toBe(fontPx('body') - 0.5);
    expect(SVG_TEXT.reading).toBe(fontPx('body'));
  });

  // R4 set the three smallest roles at one size: in order, never smaller than the one before
  it('keeps the roles in size order', () => {
    const sizes = Object.values(SVG_TEXT);
    for (let i = 1; i < sizes.length; i++) expect(sizes[i]).toBeGreaterThanOrEqual(sizes[i - 1]);
  });
});

describe('what follows from a size', () => {
  it('centres digits on a line 0.3 em under it', () => {
    expect(centreShift(10)).toBe(3);
    expect(centreShift(SVG_TEXT.tick)).toBe(3.6);
  });

  // readability F3 (the figures' text budgets): a figure stacks its words in tight lines
  it('stands a tight line on its baseline: the line less its descent over it', () => {
    for (const px of Object.values(SVG_TEXT))
      expect(lineAscentPx(px) + descentPx(px)).toBeCloseTo(tightLinePx(px), 10);
    expect([12, 12.5, 13, 13.5, 14].map(lineAscentPx)).toEqual([11.4, 11.75, 12.1, 12.45, 12.8]);
  });
});

describe('wrapWords: an SVG text broken into lines that fit', () => {
  it('keeps a text that fits on one line', () => {
    expect(wrapWords('which VM runs a script', 12, 1000)).toEqual(['which VM runs a script']);
  });

  it('breaks between words, as many to a line as fit at 0.6 em', () => {
    // 49 characters at 12 px are 352.8 px; a 328 px plot takes 45 characters a line
    const hint = 'dots: when each known script version was deployed';
    expect(wrapWords(hint, 12, 328)).toEqual([
      'dots: when each known script version was',
      'deployed',
    ]);
    for (const line of wrapWords(hint, 12, 328)) expect(line.length * 7.2).toBeLessThanOrEqual(328);
  });

  it('gives a word longer than the room a line of its own', () => {
    expect(wrapWords('a verylongword b', 12, 50)).toEqual(['a', 'verylongword', 'b']);
    expect(wrapWords('', 12, 50)).toEqual([]);
  });
});

// Review F-B (logic findings 1 and 2): a plot's name made of phrases breaks between them first,
// so every line it takes is measured and the band over the plot grows a line at a time.
describe('wrapPhrases: a name of phrases broken into lines that fit', () => {
  const phrases = ['CKB deposited', 'every complete day since 2019-05-24'];

  it('keeps the phrases on one line, joined by " · ", where the whole fits', () => {
    // 51 characters at 12 px: 367.2 px
    expect(wrapPhrases(phrases, 12, 367.2)).toEqual([
      'CKB deposited · every complete day since 2019-05-24',
    ]);
  });

  it('starts each phrase on a line of its own where the whole does not fit', () => {
    // 35 characters, 252 px, fit 294 (the deposited plot at 358 px)
    expect(wrapPhrases(phrases, 12, 367.1)).toEqual([
      'CKB deposited',
      'every complete day since 2019-05-24',
    ]);
    expect(wrapPhrases(phrases, 12, 294)).toEqual([
      'CKB deposited',
      'every complete day since 2019-05-24',
    ]);
  });

  it('wraps a phrase by its words where it does not fit a line', () => {
    // the deposited plot at a 320 px window: 288 − 48 − 16 = 224 px hold 31 characters
    expect(wrapPhrases(phrases, 12, 224)).toEqual([
      'CKB deposited',
      'every complete day since',
      '2019-05-24',
    ]);
    // the network map at 270 px: 262 px hold 36 characters, the whole name is 40
    expect(wrapPhrases(['verified nodes', 'approximate GeoIP place'], 12, 262)).toEqual([
      'verified nodes',
      'approximate GeoIP place',
    ]);
  });

  it('refuses a word wider than the room', () => {
    // "approximate" is 11 characters, 79.2 px at 12 px
    expect(() => wrapPhrases(['verified nodes', 'approximate GeoIP place'], 12, 79)).toThrow(
      /"approximate" is 79\.2 px at 12 px, wider than its 79 px/
    );
  });
});
