import { fontPx, textPx } from '@/lib/type-metrics';

/**
 * What a tab row holds at its width (UI review 2026-11-14, phase C, lane C1; the controls board the
 * owner passed on 2026-11-19, gate D0′; the rule in docs/RESPONSIVE_LAYOUT.md "Touch" and the
 * board's `tabRowPx`). A tab row (components/ui/tabs.tsx) is one line: the names at the section
 * step from sm (17 px) and the body step below it (14 px), each count at the meta step (12 px)
 * after its name, and, when the row is a tier-1 section's command line, the prompt square before
 * the first name. The owner's phone rule: when the row is wider than its line the counts leave the
 * row (the list's own count line keeps them); a name is never cut, and the row never scrolls or
 * wraps. The row is measured from what it writes (lib/type-metrics.ts, a glyph 0.6 em) and the
 * insets below, which the component draws as classes; its test reads the drawn row through
 * __tests__/utils/tailwind-box.ts against `tabRowPx`, so the two cannot drift.
 *
 * The names alone must hold the narrowest line, a 360 px window's 328 px, at the body step: a row
 * that does not is a design fault its caller must see, so `tabRowShowsCounts` throws for it at
 * every width, not only on a phone. The board's two cases: the address page's three tabs with
 * their counts are 356.4 px, so at 360 the counts leave (316.8 px without them) and at 390 (358 px)
 * they stay; the inventory's are 324 px and stay at 360, one more digit in a count and they leave.
 */

/** The two steps a tab row is set at: the section step from sm, the body step below it. */
export type TabRowStep = 'section' | 'body';

/** A tab as the row writes it: its name, and its count when it has one. */
export interface TabRowItem {
  label: string;
  count?: number | null;
}

/** The gap between two tabs, px (`gap-4 sm:gap-[22px]` in components/ui/tabs.tsx). */
const GAP_PX: Record<TabRowStep, number> = { body: 16, section: 22 };

/** The prompt square (`.section-mark`, 8 px), pulled toward the first name (`-mr-2 sm:-mr-3`). */
const SQUARE_PX = 8;
const SQUARE_PULL_PX: Record<TabRowStep, number> = { body: 8, section: 12 };

/** A count's margin after its name (`ml-1.5`), and the step it is set at (`text-meta`). */
const COUNT_GAP_PX = 6;
const COUNT_PX = fontPx('meta');

/** The line of a 360 px window (its content less the 16 px gutters): the names must hold it. */
export const NARROWEST_LINE_PX = 328;

/** A sum of widths to the hundredth of a pixel the budgets are written in. */
const hundredth = (px: number) => Math.round(px * 100) / 100;

/** A tab's count as the row writes it: grouped (`5,653`); a zero is a count too. */
export function tabCountText(count: number): string {
  if (!Number.isSafeInteger(count) || count < 0)
    throw new Error(`tab row: ${count} is not a count`);
  return count.toLocaleString('en-US');
}

/** The widths of a tab row at a step: its names, its counts, the square and the gaps. */
export interface TabRowWidths {
  names: number;
  counts: number;
  chrome: number;
  /** The row with its counts. */
  withCounts: number;
  /** The row without them. */
  bare: number;
}

/** The width a tab row's parts take at `step`, with the prompt square when it is a command line. */
export function tabRowPx(
  items: readonly TabRowItem[],
  step: TabRowStep,
  command: boolean
): TabRowWidths {
  const namePx = fontPx(step);
  const names = items.reduce((sum, item) => sum + textPx([...item.label].length, namePx), 0);
  const counts = items.reduce(
    (sum, item) =>
      item.count === undefined || item.count === null
        ? sum
        : sum + textPx(tabCountText(item.count).length, COUNT_PX) + COUNT_GAP_PX,
    0
  );
  const chrome =
    (command ? SQUARE_PX - SQUARE_PULL_PX[step] + GAP_PX[step] : 0) +
    Math.max(items.length - 1, 0) * GAP_PX[step];
  return {
    names: hundredth(names),
    counts: hundredth(counts),
    chrome: hundredth(chrome),
    withCounts: hundredth(names + counts + chrome),
    bare: hundredth(names + chrome),
  };
}

/**
 * Whether a tab row draws its counts in a line `linePx` wide at `step`: while the row with them
 * holds the line; before the line is measured (`null`, the render before the layout effect) it
 * draws them. Throws when the names alone do not hold the narrowest line at the body step.
 */
export function tabRowShowsCounts(
  items: readonly TabRowItem[],
  step: TabRowStep,
  command: boolean,
  linePx: number | null
): boolean {
  const narrow = tabRowPx(items, 'body', command);
  if (narrow.bare > NARROWEST_LINE_PX)
    throw new Error(
      `tab row "${items.map((item) => item.label).join(' · ')}" is ${narrow.bare} px without its counts at ${fontPx('body')} px: wider than the ${NARROWEST_LINE_PX} px line of a 360 px window`
    );
  if (linePx === null) return true;
  return tabRowPx(items, step, command).withCounts <= linePx;
}
