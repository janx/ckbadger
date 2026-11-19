'use client';

import { formatCount } from '@/lib/format-count';

/**
 * The one control of a fold that opens a list in place (review of phases C and D, SHOULD 5; lane
 * P1): one button that stays mounted while the list opens and folds, so it keeps the keyboard's
 * focus, and says which way it goes in both states — `aria-expanded`, `aria-controls` naming the
 * list it opens. Two components swapped at one place (the Pager's part and a second "fewer ↑"
 * button) unmounted the pressed button and dropped focus to the page.
 *
 * Its two forms, by where the list stands:
 * - `box` (the default), a list's way to go UNDER it, in the words the rows' fold and the short
 *   list's form use: folded, the whole list's count, "all 158 cells ↓" (the Pager's `more` form, as
 *   the placement lane ruled: "↓" opens in place); opened, the way back, "fewer ↑". The Pager's
 *   box: celadon words in a thumb's box, 32 px below 640 and 28 px from it. A list the route sends
 *   no count of (a collection's activity before its count is derived) folds too, asked for by name,
 *   `uncounted` (lane P2's third round, item 9): "all activity ↓", no number where there is none;
 *   the count stays required of a caller that has one.
 * - `inline` (the lead's final round, item 3), the rest of a list INSIDE a line of names (a head's
 *   calls, a participant's objects): folded, how many are folded, "+3 more locks", or the words of
 *   a figure that stands for them (`words`, a phone line's "+1 −1 objects"); opened, "fewer". It is
 *   a word of its line: the line's own size and line height (it sets neither), the fold's celadon
 *   ink, no box, so the row's line keeps its height. Its hit area is its own, not the line's: a box
 *   centred on its words (its `::before`), at least 32 px high and wide below 640 and 24 px from
 *   it, laid over the line without moving it.
 *
 * It is a control of the library (`__tests__/lib/controls-gate.test.ts`, CONTROL_FILES): the words a
 * hand-built fold writes are this file's own here.
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
        form?: 'box';
        /** The whole list's count, its noun's forms: `all 158 cells ↓`. */
        count: number;
        singular: string;
        plural?: string;
      }
    | {
        form?: 'box';
        /**
         * A list the route sends no count of, asked for by name (lane P2's third round, item 9): its
         * name, `all activity ↓`, no number where there is none. A caller that has a count gives it.
         */
        uncounted: string;
      }
    | {
        form: 'inline';
        /** How many the fold holds: `+3 more locks`. */
        count: number;
        singular: string;
        plural?: string;
        /** The folded words of a figure that stands for the folded list. */
        words?: string;
      }
  )
) {
  const { open, controls, onToggle } = props;
  if (props.form === 'inline') {
    const { count, singular, plural, words } = props;
    return (
      <button
        type="button"
        data-fold-toggle="inline"
        aria-expanded={open}
        aria-controls={controls}
        onClick={() => onToggle(!open)}
        className="text-jade relative whitespace-nowrap bg-transparent p-0 underline-offset-4 before:absolute before:left-1/2 before:top-1/2 before:h-8 before:w-full before:min-w-8 before:-translate-x-1/2 before:-translate-y-1/2 before:content-[''] hover:underline sm:before:h-6 sm:before:min-w-6"
      >
        {open
          ? 'fewer'
          : (words ?? `+${formatCount(count, `more ${singular}`, plural && `more ${plural}`)}`)}
      </button>
    );
  }
  if ('words' in props)
    throw new Error(`a box fold writes its own words (all … ↓), not "${String(props.words)}"`);
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
