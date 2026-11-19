import { groupCount } from '@/lib/format-count';
import { fontPx, foreignTextPx, textPx } from '@/lib/type-metrics';

/**
 * What a tab row holds at its width (UI review 2026-11-14, phase C, lane C1; the controls board the
 * owner passed on 2026-11-19, gate D0′; the board's `tabRowPx`; docs/RESPONSIVE_LAYOUT.md's
 * principle 4, the measured width decides). A tab row (components/ui/tabs.tsx) is one line: the
 * names at the section step from sm (17 px) and the body step below it (14 px) for a command row,
 * at the body step at every width for a plain one (components/ui/tabs.tsx `TAB_STEP`), each count at the
 * meta step (12 px) after its name, and, when the row is a tier-1 section's command line, the
 * prompt square before the first name. The owner's phone rule: when the row is wider than its line
 * the counts leave the row (the list's own count line keeps them); a name is never cut, and the
 * row never scrolls or wraps. The row is measured from what it writes (lib/type-metrics.ts, a glyph 0.6 em) and the
 * insets below, which the component draws as classes; its test reads the drawn row through
 * __tests__/utils/tailwind-box.ts against `tabRowPx`, so the two cannot drift.
 *
 * The names alone must hold the narrowest line, a 360 px window's 328 px, at the body step: a row
 * that does not is a design fault its caller must see, so `tabRowShowsCounts` throws for it at
 * every width, not only on a phone. The board's two cases: the address page's three tabs with
 * their counts are 356.4 px, so at 360 the counts leave (316.8 px without them) and at 390 (358 px)
 * they stay; the inventory's are 324 px and stay at 360, one more digit in a count and they leave.
 *
 * A row of filter chips is measured the same way (`chipFormFits`, below): below 640 a filter's
 * values are chips where their row fits one line of the narrowest phone, a select where it does not,
 * decided from its labels and marks with room for its counts, never from the counts themselves.
 */

/** The two steps a tab row is set at: a command row the section step from sm, the body step below it; a plain row the body step. */
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
        : sum + textPx(groupCount(item.count).length, COUNT_PX) + COUNT_GAP_PX,
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

/** What a tab row draws on its line: its tabs with their counts, its tabs alone, or the Select. */
export type TabRowForm = 'counts' | 'names' | 'select';

/**
 * A tab row's form on a line `linePx` wide at `step` (the lead's final round, item 4): its tabs with
 * their counts while the row with them holds the line; its tabs alone (the counts leave first, the
 * owner's phone rule) while its names do; else its narrow form, the library's Select, which never
 * clips, wraps or scrolls a row. Decided from its labels (and its counts, which leave first), as a
 * filter's chips are (`chipFormFits`). Before the line is measured (`null`) the row with its
 * counts. The cases: the transactions and address rows, 316.8 px without counts at the body step,
 * are the Select in a 320 px window's 288 px line and tabs in a 360 px window's 328.
 */
export function tabRowForm(
  items: readonly TabRowItem[],
  step: TabRowStep,
  command: boolean,
  linePx: number | null
): TabRowForm {
  if (linePx === null) return 'counts';
  const row = tabRowPx(items, step, command);
  if (row.withCounts <= linePx) return 'counts';
  return row.bare <= linePx ? 'names' : 'select';
}

/** A filter chip as the row writes it: its label, its count and its mark where it has them. */
export interface ChipRowItem {
  label: string;
  count?: number | null;
  /** The palette key of its 7 px mark; only whether it has one bears on the row. */
  mark?: string;
}

/**
 * A chip's box (`chipClass`, components/ui/filter-chips.tsx): its 1 px frame and its 10 px padding
 * each side (`border px-2.5`), the gap between its parts (`gap-1.5`), its mark (`h-[7px] w-[7px]`),
 * its label at the small step (`text-small`, 13 px), its count at the meta step; the gap between
 * two chips (`gap-1.5` on the row).
 */
const CHIP_INSET_PX = 2 * 1 + 2 * 10;
const CHIP_PART_GAP_PX = 6;
const CHIP_MARK_PX = 7;
const CHIP_LABEL_PX = fontPx('small');
const CHIP_GAP_PX = 6;

/** A row of chips, each its label, its mark where it has one and its count's glyphs (or none). */
function chipsPx(chips: readonly { label: string; mark?: string; countGlyphs: number | null }[]) {
  const sum = chips.reduce(
    (total, chip) =>
      total +
      CHIP_INSET_PX +
      (chip.mark === undefined ? 0 : CHIP_MARK_PX + CHIP_PART_GAP_PX) +
      textPx([...chip.label].length, CHIP_LABEL_PX) +
      (chip.countGlyphs === null ? 0 : CHIP_PART_GAP_PX + textPx(chip.countGlyphs, COUNT_PX)),
    0
  );
  return hundredth(sum + Math.max(chips.length - 1, 0) * CHIP_GAP_PX);
}

/** The width a row of filter chips takes on one line as written, to the hundredth of a pixel. */
export function chipRowPx(items: readonly ChipRowItem[]): number {
  return chipsPx(
    items.map((item) => ({
      label: item.label,
      mark: item.mark,
      countGlyphs:
        item.count === undefined || item.count === null ? null : groupCount(item.count).length,
    }))
  );
}

/**
 * The glyphs of room a filter that counts its values leaves for each count when its phone form is
 * decided (lane cd-fixes-controls, L1 NOTE 11): five, a count up to 9,999 as written, the day's
 * counts of the kind filters (the board's 6,122 and 5,425) and the catalog's (727).
 */
export const CHIP_COUNT_CHARS = 5;

/**
 * The width a filter's row of chips takes on one line as its phone form is decided: its labels and
 * marks as written, and, where the filter counts its values (`counted`), a count of
 * CHIP_COUNT_CHARS glyphs on every value, whether its count has come, is narrower, or is none. A
 * count on a filter that does not count its values is refused: its form left it no room.
 */
export function chipFormPx(items: readonly ChipRowItem[], counted: boolean): number {
  return chipsPx(
    items.map((item) => {
      if (!counted && item.count !== undefined && item.count !== null)
        throw new Error(`a count (${item.count}) on a filter that does not count its values`);
      return { label: item.label, mark: item.mark, countGlyphs: counted ? CHIP_COUNT_CHARS : null };
    })
  );
}

/**
 * Whether a filter's values are chips below 640 (the controls board's pictures at 390): when their
 * row with room for its counts (`chipFormPx`) fits one line of the narrowest phone
 * (`NARROWEST_LINE_PX`, a 360 px window's 328 px); else a select after the filter's label. It is
 * decided from what does not change with the data, its labels, its marks and whether it counts, so
 * one filter takes one form on every page and every phone, before and after its counts come (L1
 * NOTE 11: from the counts, the four standards were chips while they loaded and a select when they
 * came). A fixed line, not the one measured. From 640 they are chips. The cases, with room: the
 * address page's `all · ckb · dao` 287.2 px, chips; the inventory's four standards 476.8 px and
 * the six kinds of activity 876.8 px, selects.
 */
export function chipFormFits(items: readonly ChipRowItem[], counted: boolean): boolean {
  return chipFormPx(items, counted) <= NARROWEST_LINE_PX;
}

/**
 * Whether a row of chips below 640 draws its counts: while the row as written holds the narrowest
 * line. A count wider than its room (more than CHIP_COUNT_CHARS glyphs) can push the row past it;
 * then the counts leave the row, as they leave a tab row (the owner's phone rule): a chip is never
 * cut and the row never takes a second line. The count of the chosen value stays with the list it
 * filters.
 */
export function chipRowShowsCounts(items: readonly ChipRowItem[]): boolean {
  return chipRowPx(items) <= NARROWEST_LINE_PX;
}

/**
 * A select (components/ui/select.tsx): its label at the small step (`text-small`, 13 px), the gap
 * after it (`gap-2.5`), and its well: the field's 1 px frame each side (FIELD_WELL, its own padding
 * taken off: `px-0`), the select's 10 px before its value (`pl-2.5`) and 25 px after it, where the
 * chevron stands (`pr-[25px]`), the value at the small step.
 */
const SELECT_TEXT_PX = fontPx('small');
const SELECT_GAP_PX = 10;
const SELECT_WELL_INSET_PX = 2 * 1 + 10 + 25;

/** The widths of a select's pair: its label, its well with its widest option, and the two. */
export interface SelectPairWidths {
  label: number;
  well: number;
  /** The label, the gap and the well on one line. */
  pair: number;
}

/**
 * The width a select's label and well take on one line. A native select is as wide as its widest
 * option, which an option's words set: measured as text that is not ours (lib/type-metrics.ts
 * `foreignTextPx`: a name may hold any script), exactly the served advance for a Latin one.
 */
export function selectPairPx(label: string, options: readonly string[]): SelectPairWidths {
  if (options.length === 0) throw new Error(`select "${label}": no option to measure`);
  const labelPx = textPx([...label].length, SELECT_TEXT_PX);
  const widest = Math.max(...options.map((option) => foreignTextPx(option, SELECT_TEXT_PX)));
  const well = widest + SELECT_WELL_INSET_PX;
  return {
    label: hundredth(labelPx),
    well: hundredth(well),
    pair: hundredth(labelPx + SELECT_GAP_PX + well),
  };
}

/**
 * Whether a select's well goes under its label in a line `linePx` wide (lane c2-finish, item 2: a
 * select never cuts its value): when its pair is wider than the line; before the line is measured
 * (`null`) the pair stands on one line. The well never shrinks, so a native select never clips its
 * chosen value; an option that makes the well wider than the narrowest line (a 360 px window's
 * 328 px) even alone is a naming fault its caller must see, so this throws for it at every width.
 * The cases: the object page's `version` · `12 · #21,016,901 · current` is 304.4 px, on one line
 * at 360; with the label `cell version` it is 343.4 px, under it at 360 and on one line at 390.
 */
export function selectWellUnderLabel(
  label: string,
  options: readonly string[],
  linePx: number | null
): boolean {
  const widths = selectPairPx(label, options);
  if (widths.well > NARROWEST_LINE_PX) {
    const widest = options.reduce((a, b) =>
      foreignTextPx(b, SELECT_TEXT_PX) > foreignTextPx(a, SELECT_TEXT_PX) ? b : a
    );
    throw new Error(
      `select "${label}": its option "${widest}" makes a well ${widths.well} px wide, wider than the ${NARROWEST_LINE_PX} px line of a 360 px window`
    );
  }
  if (linePx === null) return false;
  return widths.pair > linePx;
}
