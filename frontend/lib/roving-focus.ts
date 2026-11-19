/**
 * The keyboard of a row of controls that is one stop in the tab order (WAI-ARIA's roving
 * tabindex; the controls lane C1): the tab row (components/ui/tabs.tsx) moves along its line with
 * ← and →; a radio group (the filter chips, the range) with any arrow, ← ↑ back and → ↓ on. Both
 * wrap at their ends, and Home and End go to the first and the last.
 */

/** Which arrows move a row: a tab row's line (← →), or any arrow (a radio group). */
export type RovingAxis = 'line' | 'any';

/** The index a key moves the focus to from `index` in a row of `count`; null for a key not its own. */
export function rovingTarget(
  key: string,
  index: number,
  count: number,
  axis: RovingAxis
): number | null {
  if (count < 1) throw new Error('roving focus: a row with no item');
  if (!Number.isInteger(index) || index < 0 || index >= count)
    throw new Error(`roving focus: ${index} is not in a row of ${count}`);
  const back = key === 'ArrowLeft' || (axis === 'any' && key === 'ArrowUp');
  const on = key === 'ArrowRight' || (axis === 'any' && key === 'ArrowDown');
  if (back) return (index - 1 + count) % count;
  if (on) return (index + 1) % count;
  if (key === 'Home') return 0;
  if (key === 'End') return count - 1;
  return null;
}
