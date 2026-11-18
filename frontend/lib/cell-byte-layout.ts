/**
 * Geometry of the cell page's byte microscope: how many whole bytes a hex-dump row holds in the
 * measured box, and how narrow a part of the capacity map may be. The microscope's units are drawn
 * at full size; a narrower box shows fewer bytes a row, never smaller ones.
 */

import { charPx, withAllowance } from '@/lib/type-metrics';

/** The advance of one glyph of the dump, set at the small step (`text-small`). */
const DUMP_CHAR_PX = charPx('small');
/** One hex byte: two glyphs plus its 1 px padding on each side (`px-px`). */
const HEX_CELL_PX = 2 * DUMP_CHAR_PX + 2;
/** The gap between two hex bytes (`gap-1`). */
const HEX_GAP_PX = 4;
/** The ASCII column's margin, rule and padding (`ml-[18px] border-l pl-3.5`). */
const ASCII_INSET_PX = 18 + 1 + 14;
/** The offset column: `w-[62px]` for 0x0000, `w-[74px]` for 0x00000. */
const OFFSET_PX = { 4: 62, 5: 74 } as const;
/** The dump box's padding and border (`px-4`, 1 px border) around its rows. */
export const HEX_BOX_INSET_PX = 2 * 16 + 2;
/** Row lengths the dump uses, widest first; each divides the 24-byte excerpt alignment. */
const ROW_BYTES = [24, 16, 8] as const;
export type HexRowBytes = (typeof ROW_BYTES)[number];

/** The drawn width of one dump row of `n` bytes: offset, hex bytes, ASCII column. */
export function hexRowWidth(n: number, digits: 4 | 5): number {
  return (
    OFFSET_PX[digits] + n * HEX_CELL_PX + (n - 1) * HEX_GAP_PX + ASCII_INSET_PX + n * DUMP_CHAR_PX
  );
}

/**
 * The most bytes a row holds whole in a dump box `boxWidth` px wide: 24, else 16, else 8. Below
 * the 8-byte row's width (a 360 px phone) the 8-byte row stays and the raw bytes scroll inside
 * their own box, as raw data may.
 */
export function hexRowBytes(boxWidth: number, digits: 4 | 5): HexRowBytes {
  if (!(boxWidth > 0)) throw new Error(`invalid cell byte box width ${boxWidth}`);
  return ROW_BYTES.find((n) => hexRowWidth(n, digits) + HEX_BOX_INSET_PX <= boxWidth) ?? 8;
}

/**
 * The capacity map labels' allowance: the advance below was measured for the 10.5 px label the map
 * had before the type scale (6.3 px a glyph), and the label is `text-meta` (11 px, 6.6) since; the
 * budget kept its number, so it counts 0.3 px a glyph under the label it holds.
 */
const MAP_LABEL_ALLOWANCE = -0.3;
/** The advance a capacity-map label's minimum width is measured at, and the label's side padding. */
export const MAP_CHAR_PX = withAllowance(charPx('meta'), MAP_LABEL_ALLOWANCE);
export const MAP_PAD_PX = 4;

/**
 * The narrowest a capacity-map part may be: wide enough to hold its label whole inside its
 * padding, and at least `floor`. The map lays its parts out as a flex row with basis 0, growth by
 * bytes and this minimum, so the gaps sit inside the row's width, a part too small for its share
 * keeps its minimum, and the rest of the row is shared in proportion to bytes (responsive A#10:
 * a 2-byte part was an 8 px sliver round its "2").
 */
export function mapSegmentMinWidth(label: string, floor: number): number {
  if (!label) throw new Error('capacity map part has no label');
  return Math.max(floor, label.length * MAP_CHAR_PX + 2 * MAP_PAD_PX);
}
