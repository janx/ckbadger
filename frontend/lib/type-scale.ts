/**
 * The site's one type scale (hierarchy plan 2026-11-02, ruling R1): eight steps, written as
 * `text-<step>`. `tailwind.config.ts` registers them as its `fontSize` theme, and `cn()` tells
 * tailwind-merge they are sizes, so a class list can carry a size step and a colour together.
 *
 * - meta 12 · labels, axis ticks, section notes, eyebrows; never prose (readability R4: no
 *   information under 12 px, and 12 px only for a label, a tick or a note)
 * - small 13 · readout rows, participants, legends
 * - body 14 · captions, table cells, feed headlines, running text and tier-2 prose
 * - section 17 · h2 in tier 1
 * - hero 22 · metric values in a hero grid, chart readout figures
 * - index 26 · h1 of a topic or list page
 * - entity 30 · h1 of an entity page (address, tx, cell, script, token, object, identity)
 * - display 40 · the one figure of a topic page
 */
export const TYPE_STEPS = [
  'meta',
  'small',
  'body',
  'section',
  'hero',
  'index',
  'entity',
  'display',
] as const;

export type TypeStep = (typeof TYPE_STEPS)[number];

/** `[font-size, line-height]` per step, in the order of {@link TYPE_STEPS}. */
export const TYPE_SCALE: Record<TypeStep, [fontSize: string, lineHeight: string]> = {
  meta: ['12px', '1.5'],
  small: ['13px', '1.5'],
  body: ['14px', '1.6'],
  section: ['17px', '1.3'],
  hero: ['22px', '1.15'],
  index: ['26px', '1.2'],
  entity: ['30px', '1.15'],
  display: ['40px', '1.05'],
};
