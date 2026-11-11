import { useMemo } from 'react';
import { color } from '@/lib/palette';
import {
  assertSealId,
  drawSeal,
  type SealForm,
  type SealId,
  type SealState,
} from '@/lib/seal/stones';
import { cn } from '@/lib/utils';

export interface SealProps {
  /** What the seal is of: a block, a transaction, a cell, or a script (a lock is an address). */
  id: SealId;
  /** `page`: the whole stone, red text (an object page's head); `row`: the white-text stone. */
  form: SealForm;
  /** The seal's height in px; its width follows from the kind and the form. */
  height: number;
  /**
   * `pressed`, in force (the cinnabar); `ink`, a consumed cell or an uncle (the dead ink);
   * `unpressed`, not yet committed (a dashed stone, its field at half strength).
   */
  state?: SealState;
  /** Degrees. */
  tilt?: number;
  /** Pressed across a seam: the gap between two sheets shows through, this many px wide. */
  seamPx?: number;
  className?: string;
}

/**
 * The three inks: the cinnabar `mark` (UI grade, both themes: the prompt square's red, never the
 * text red `rouge`), the page ground a row stone's text is cut in, the dead ink.
 */
const PAINTS = {
  ink: color('mark'),
  ground: color('base-bg'),
  dead: color('text-dim'),
} as const;

/**
 * The fields of a checked id as one string: two ids with the same fields draw the same seal, so
 * the drawing is kept by its fields and not by the object's identity (a list re-rendered with
 * fresh objects draws nothing again). Every field is a validated hash, a whole number, `null`, a
 * class name or a BigInt, none with a space in it, so two different ids never share a key.
 */
function keyOf(id: SealId): string {
  switch (id.kind) {
    case 'block':
      return `block ${id.hash} ${id.txs}`;
    case 'tx':
      return `tx ${id.hash} ${id.consumed} ${id.created}`;
    case 'cell':
      return `cell ${id.tx} ${id.index} ${id.lock} ${id.type === null ? '-' : `${id.type.class} ${id.type.hash}`} ${id.capacity}`;
    case 'script':
      return `script ${id.hash}`;
  }
}

/**
 * The seal of a block, a transaction, a cell or a script (`lib/seal`): the outline says what it
 * is, the marks say which one and what it holds; the same id always the same seal.
 *
 * The id is checked on every render (an id that cannot be drawn throws, naming it); the drawing
 * is made once per id's fields, form, height, state, tilt and seam. The SVG is the generator's
 * own markup — numbers, fixed element names, validated hashes, whole numbers, a class name and the
 * three validated paints — so it is set as inner HTML with nothing to sanitise.
 */
export function Seal({ id, form, height, state = 'pressed', tilt, seamPx, className }: SealProps) {
  assertSealId(id, form);
  const key = keyOf(id);
  // the markup object itself is memoised: React sets inner HTML again whenever the object changes
  const markup = useMemo(
    () => ({ __html: drawSeal(id, { form, height, state, ...PAINTS, tilt, seamPx }).svg }),
    // `id` by its fields (`key`), not by its identity
    [key, form, height, state, tilt, seamPx]
  );
  return (
    <span
      data-seal={id.kind}
      data-seal-form={form}
      data-seal-state={state}
      className={cn('inline-block shrink-0 leading-none', className)}
      dangerouslySetInnerHTML={markup}
    />
  );
}
