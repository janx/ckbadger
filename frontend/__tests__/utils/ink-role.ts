import { TYPE_STEPS } from '@/lib/type-scale';

/**
 * The ink a word is read in, by its role (readability ruling R3; the colour guidelines' "Text
 * hierarchy"): the bright ink for the one figure or name scanned first, the reading ink for every
 * other value, link, date, number and explaining sentence, the quiet ink for a label, a repeated
 * unit, a separator, a note and a set-back row.
 */
export const BRIGHT = 'text-text-bright';
export const READING = 'text-text';
export const QUIET = 'text-text-dim';

/** `text-<x>` classes that are not an ink: the type steps and the alignments. */
const NOT_INK = new Set<string>([
  ...TYPE_STEPS,
  'left',
  'right',
  'center',
  'justify',
  'start',
  'end',
  'wrap',
  'nowrap',
  'balance',
  'pretty',
  'ellipsis',
  'clip',
]);

/** An element's own ink at rest: a `text-<ink>` class with no variant, or an inline colour. */
function ownInk(el: Element): string | null {
  const style = (el as HTMLElement).style?.color;
  if (style) return style;
  for (const c of Array.from(el.classList)) {
    if (!c.startsWith('text-')) continue;
    const key = c.slice('text-'.length);
    if (NOT_INK.has(key)) continue;
    if (key.startsWith('[') && !key.includes('color:')) continue;
    return c;
  }
  return null;
}

/**
 * The ink an element's words are drawn in, as its classes say (jsdom applies no stylesheet):
 * `quiet` inside a set-back row (the `quiet` class draws every word in it, but the ghost's padding,
 * in the quiet ink, whatever their own classes: app/globals.css); else its own ink, else the
 * nearest ancestor's; `none` where nothing sets one (the body's bright).
 */
export function inkOf(el: Element): string {
  if (el.closest('.quiet') && !el.classList.contains('text-text-ghost')) return 'quiet';
  for (let e: Element | null = el; e; e = e.parentElement) {
    const ink = ownInk(e);
    if (ink !== null) return ink;
  }
  return 'none';
}
