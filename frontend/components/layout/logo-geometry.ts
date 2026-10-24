/**
 * Where the header badge sits. It is absolutely positioned and tilted 8°, hanging across the header
 * border; the header row and the vitals strip reserve a notch for it so it never covers text.
 */

/** The badge width: ~96 px on a phone, growing with the viewport to 157 px on a desktop. */
export const LOGO_WIDTH_CLASS = 'w-[clamp(96px,11vw,157px)]';

/** The header row's notch: the badge's width minus the part its tilt leaves free. */
export const LOGO_NOTCH_CLASS = 'w-[calc(clamp(96px,11vw,157px)_-_17px)] shrink-0';

/** The vitals strip's notch from md up: the badge's width plus a gap (below md it ends above). */
export const LOGO_STRIP_NOTCH_CLASS =
  'hidden md:block md:w-[calc(clamp(96px,11vw,157px)_+_15px)] md:shrink-0';
