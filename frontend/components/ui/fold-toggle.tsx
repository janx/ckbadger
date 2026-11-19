'use client';

import { formatCount } from '@/lib/format-count';

/**
 * The one control of a fold that opens a list in place (review of phases C and D, SHOULD 5; lane
 * P1): one button that stays mounted while the list opens and folds, so it keeps the keyboard's
 * focus, and says which way it goes in both states — `aria-expanded`, `aria-controls` naming the
 * list it opens — in the words the rows' fold and the short list's form use: folded, the whole
 * list's count, "all 158 cells ↓" (the Pager's `more` form, as the placement lane ruled: "↓" opens
 * in place); opened, the way back, "fewer ↑". Two components swapped at one place (the Pager's part
 * and a second "fewer ↑" button) unmounted the pressed button and dropped focus to the page.
 *
 * It is a control of the library (`__tests__/lib/controls-gate.test.ts`, CONTROL_FILES): the words a
 * hand-built pager writes are this file's own here. Its users: the transaction structure's sheets
 * (`components/transaction/tx-structure.tsx`) and the transaction page's empty witnesses; the rows'
 * fold of a feed (`components/activity/activity-row.tsx`, the lists lane's) is the third to convert.
 *
 * A list the route sends no count of (a collection's activity before its count is derived) folds
 * too, asked for by name, `uncounted` (lane P2's third round, item 9): "all activity ↓", no number
 * where there is none; the count stays required of a caller that has one.
 */
export function FoldToggle(
  props: {
    /** Whether the list is drawn whole. */
    open: boolean;
    /** The id of the element the fold opens and folds. */
    controls: string;
    onToggle: (open: boolean) => void;
  } & (
    | {
        /** The whole list's count, its noun's forms: `all 158 cells ↓`. */
        count: number;
        singular: string;
        plural?: string;
      }
    | {
        /**
         * A list the route sends no count of, asked for by name (lane P2's third round, item 9): its
         * name, `all activity ↓`, no number where there is none. A caller that has a count gives it.
         */
        uncounted: string;
      }
  )
) {
  const { open, controls, onToggle } = props;
  const whole =
    'uncounted' in props ? props.uncounted : formatCount(props.count, props.singular, props.plural);
  return (
    <button
      type="button"
      data-fold-toggle=""
      aria-expanded={open}
      aria-controls={controls}
      onClick={() => onToggle(!open)}
      // the Pager's way to go: celadon words in a thumb's box, 32 px below 640 and 28 px from it
      className="text-jade text-small inline-flex h-8 items-center whitespace-nowrap underline-offset-4 hover:underline sm:h-7"
    >
      {open ? 'fewer ↑' : `all ${whole} ↓`}
    </button>
  );
}
