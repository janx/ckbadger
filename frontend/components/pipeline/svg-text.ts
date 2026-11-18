/** SVG text styles of the pipeline canvas (kept under components/ so Tailwind sees them). */
export const T10 = 'fill-text-dim text-meta';
export const T10B = 'fill-text text-meta';
export const T10C = 'fill-text-bright text-meta';
export const T10G = 'fill-gold text-meta';
export const T11C = 'fill-text-bright text-meta';
/**
 * A block's number on its plate, in a row and a column: 12 px / 500 in the bright ink (the boards'
 * `.no`, Plate.dc.html and PlatePhone.dc.html), inside the proposal window too, where it stands on
 * the gilt strap (7.6 / 8.4 : 1; the word gold would be 4.2 / 3.8 : 1 there).
 */
export const T12C = 'fill-text-bright text-small font-medium';
/** NEXT's number: 12 px / 500 in the dim ink, on no plate (the boards' `.nx .no`). */
export const T12D = 'fill-text-dim text-small font-medium';

/** The line under the canvas (legend or pinned selection): its content wraps whenever it is narrow. */
export const FOOTER_ROW = 'flex min-h-6 flex-wrap items-center gap-y-1 font-mono';

export const fmtInt = (n: number | bigint) => n.toLocaleString('en-US');
