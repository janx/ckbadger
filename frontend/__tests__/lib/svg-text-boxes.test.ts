import { describe, expect, it } from 'vitest';
import {
  allTextLines,
  meetings,
  pastPx,
  textLines,
  type TextLine,
} from '@/__tests__/utils/svg-text-boxes';

/**
 * The two measures a known fault of a figure is pinned by (review F-B's test finding 1): how far
 * two words overlap across, and how far a word stands past its box. Boxes are written by hand.
 */
const line = (text: string, left: number, right: number, top = 0, bottom = 15): TextLine => ({
  text,
  left,
  right,
  top,
  bottom,
  size: 12,
});

describe('meetings', () => {
  it('names every two lines that meet, with the width they share across', () => {
    const a = line('15:47:24.564', 100, 186.4);
    const b = line('15:47:24.564', 177.6, 264);
    const c = line('clear of both', 300, 393.6);
    expect(meetings([a, b, c])).toEqual([{ a: '15:47:24.564', b: '15:47:24.564', by: 8.8 }]);
  });

  it('finds none where lines stand apart, side by side or a tight line apart', () => {
    expect(meetings([line('a', 0, 10), line('b', 10.5, 20)])).toEqual([]);
    // stacked: one line's foot is the next one's top
    expect(meetings([line('a', 0, 10, 0, 15), line('b', 0, 10, 15, 30)])).toEqual([]);
  });

  it('counts a touch as a meeting of no width', () => {
    expect(meetings([line('a', 0, 10), line('b', 10, 20)])).toEqual([{ a: 'a', b: 'b', by: 0 }]);
  });
});

describe('pastPx', () => {
  const box = { left: 0, top: 0, right: 326, bottom: 100 };
  it('measures how far a line stands past the box on its farthest side, to the hundredth', () => {
    expect(pastPx(line('6 epochs', 269.48, 327.08), box)).toBe(1.08);
    expect(pastPx(line('below', 10, 20, 95, 110), box)).toBe(10);
    expect(pastPx(line('left', -4.2, 20), box)).toBe(4.2);
  });

  it('is 0 for a line flush with the box and negative inside it', () => {
    expect(pastPx(line('flush', 300, 326), box)).toBe(0);
    expect(pastPx(line('inside', 10, 20, 10, 25), box)).toBe(-10);
  });
});

// Review fb-tests SHOULD-4 (lane rd/fb-fixes-2, T4): the reader measured every run at 0.6 em a code
// point, so a CJK character counted 7.2 px at 12 px and a ZWJ emoji five glyphs, against the
// ruling for text that is not ours. It measures a run as the figures do (foreignTextPx): a
// grapheme of the served Latin subset at 0.6 em, any other at 1.25 em (15 px at 12 px).
describe('textLines measures text that is not ours as the figures do', () => {
  it('counts a CJK character and an emoji at 1.25 em, a Latin glyph at 0.6', () => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    const text = (x: number, anchor: string, words: string) => {
      const el = document.createElementNS('http://www.w3.org/2000/svg', 'text');
      el.setAttribute('x', String(x));
      el.setAttribute('y', '20');
      el.setAttribute('font-size', '12');
      el.setAttribute('text-anchor', anchor);
      el.textContent = words;
      svg.append(el);
    };
    text(10, 'start', '蚂蚁矿池 AntPool');
    text(200, 'end', '👨‍👩‍👧 ok');
    const [cjk, emoji] = allTextLines(svg);
    // four CJK characters (60) and " AntPool" (8 Latin glyphs, 57.6)
    expect(cjk.right - cjk.left).toBeCloseTo(4 * 15 + 8 * 7.2, 9);
    // one family (15) and " ok" (21.6), ending at its x
    expect(emoji.right).toBe(200);
    expect(emoji.right - emoji.left).toBeCloseTo(15 + 3 * 7.2, 9);
  });

  it('reads the face a run is drawn from: our → at 0.6 em in the regular face, not in the semibold', () => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    for (const weight of ['400', '600', '700']) {
      const el = document.createElementNS('http://www.w3.org/2000/svg', 'text');
      el.setAttribute('x', '0');
      el.setAttribute('y', '20');
      el.setAttribute('font-size', '12');
      el.setAttribute('font-weight', weight);
      el.textContent = 'a → b';
      svg.append(el);
    }
    const [regular, semibold, bold] = Array.from(svg.querySelectorAll('text'));
    // the regular face maps the arrow (U+2192); the semibold subset does not, so a fallback draws it
    expect(textLines(regular)[0].right).toBeCloseTo(5 * 7.2, 9);
    expect(textLines(semibold)[0].right).toBeCloseTo(4 * 7.2 + 15, 9);
    // no face is served at 700: the reader cannot say what draws it
    expect(() => textLines(bold)).toThrow(/no served face for 700/);
  });
});
