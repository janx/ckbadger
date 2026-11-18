import { descentPx, fontPx, textPx, tightLinePx } from '@/lib/type-metrics';

/**
 * The sizes of SVG text by role (readability plan 2026-11-18, lane F3): every `fontSize` on an
 * SVG text element reads this table (the gate is `__tests__/lib/chart-type-gate.test.ts`), and
 * the baselines and widths that follow from a size are computed from it here or beside the figure.
 *
 * Ruling R3 (hierarchy plan 2026-11-02) brought HTML onto the type scale and left a chart's own
 * text where it was, between the scale's steps. Ruling R4 (readability 2026-11-18) sets no SVG
 * text under 12 px: the three smallest roles, which stood 1 and 0.5 px under the meta step, are
 * the meta step now (12 px), and the larger roles keep their place against their own step. Every
 * role reads the scale (lib/type-scale.ts); `SVG_TEXT_FLOOR` is the floor the gate holds.
 */
export const SVG_TEXT = {
  /** 12 px, the meta step: a chart's axis ticks and rulers, and a figure's smallest marks (a tile's hash and time, a gel's lane digits, a band's label). */
  tick: fontPx('meta'),
  /** 12 px, the meta step: a second line under a name (a date, an epoch, a sub-label) and a figure's notes. */
  note: fontPx('meta'),
  /** 12 px, the meta step: a plot's name and key, a figure's labels, a time axis set at the label size. */
  label: fontPx('meta'),
  /** 12.5 px, half a pixel under the small step: a figure's figures set under or beside its labels (a count, a byte range, a box's value). */
  value: fontPx('small') - 0.5,
  /** 13 px, the small step: the line that leads an item in a figure (a tile's number, a gene's name, a lane's head, a key's words). */
  head: fontPx('small'),
  /** 13.5 px, half a pixel under the body step: a figure's titles over a rule (an edition's name). */
  title: fontPx('body') - 0.5,
  /** 14 px, the body step: a value read out over a plot (an end label, the hover reading, words in a tall box). */
  reading: fontPx('body'),
} as const;

/** No SVG text under the meta step, 12 px (readability R4). */
export const SVG_TEXT_FLOOR = fontPx('meta');
for (const [role, px] of Object.entries(SVG_TEXT))
  if (px < SVG_TEXT_FLOOR)
    throw new Error(
      `chart-type: the ${role} role is ${px} px, under the ${SVG_TEXT_FLOOR} px floor`
    );

/**
 * How far a tight line of SVG text set at `px` stands over its baseline: the line
 * (`tightLinePx`, its size and 3 px of leading) less its descent (`descentPx`, 0.3 em): 11.4 at
 * 12 px, 12.1 at 13. A figure stacks its words in tight lines: a line whose top is at `top` stands
 * on the baseline `top + lineAscentPx(px)`, its descent reaching `top + tightLinePx(px)`; a band of
 * n lines is n tight lines and the band's stated padding (readability plan 2026-11-18, lane F3).
 */
export function lineAscentPx(px: number): number {
  return tightLinePx(px) - descentPx(px);
}

/**
 * The shift from a line's y to the baseline that centres a run of digits on it: 0.3 em (3 px for
 * a 10 px tick on its gridline).
 */
export function centreShift(px: number): number {
  return (px * 3) / 10;
}

/**
 * The lines an SVG text set at `px` takes in `room` px: whole words, as many to a line as fit at
 * 0.6 em a glyph (an SVG text does not wrap by itself). A word longer than the room keeps a line of
 * its own and is the caller's to refuse; an empty text has no line.
 */
export function wrapWords(text: string, px: number, room: number): string[] {
  const lines: string[] = [];
  for (const word of text.split(' ').filter(Boolean)) {
    const last = lines.length - 1;
    if (last >= 0 && textPx(lines[last].length + 1 + word.length, px) <= room)
      lines[last] = `${lines[last]} ${word}`;
    else lines.push(word);
  }
  return lines;
}

/**
 * The lines a plot's name takes in `room` px when it is phrases joined by " · " ("CKB deposited ·
 * every complete day since …"): one line where the whole fits; else each phrase starts a line of
 * its own and wraps by its words (`wrapWords`), so the band over the plot grows a line at a time
 * and every line is measured (review F-B, logic findings 1 and 2). A word wider than the room
 * cannot be placed and is refused. The phrases are the figure's own words, measured at 0.6 em.
 */
export function wrapPhrases(phrases: readonly string[], px: number, room: number): string[] {
  const whole = phrases.join(' · ');
  if (textPx(whole.length, px) <= room) return [whole];
  const lines = phrases.flatMap((phrase) => wrapWords(phrase, px, room));
  for (const line of lines)
    if (textPx(line.length, px) > room)
      throw new Error(
        `chart-type: "${line}" is ${textPx(line.length, px)} px at ${px} px, wider than its ${room} px`
      );
  return lines;
}
